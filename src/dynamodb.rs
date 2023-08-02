use std::{
    collections::{HashMap, HashSet},
    env,
};

use async_trait::async_trait;
use aws_config::{environment::EnvironmentVariableCredentialsProvider, retry::RetryConfigBuilder};
use aws_sdk_dynamodb::{
    operation::{
        create_table::{CreateTableError, CreateTableOutput},
        put_item::PutItemError,
        update_item::UpdateItemError,
    },
    primitives::Blob,
    types::{
        AttributeDefinition, AttributeValue, BillingMode, KeySchemaElement, KeyType,
        KeysAndAttributes, PutRequest, ScalarAttributeType, WriteRequest,
    },
    Client,
};
use aws_smithy_http::result::SdkError;
use chrono::{NaiveDateTime, Utc};
use cosmian_findex::{parameters::UID_LENGTH, EncryptedTable, Uid, UpsertData};
use futures::StreamExt;

use crate::{
    core::{Index, IndexesDatabase, MetadataDatabase, NewIndex, Table},
    errors::Error,
};

/// DynamoDB implementation
///
/// Use 3 tables, one for the metadata (indexes names, keys), one for the entries
/// and one for the chains.
///
/// Entries and chains IDs are composed of the index `id` as bytes concat with
/// the UID. Maybe we could split that and use a composed index in DynamoDB? Having
/// a composed index may be useful to compute the size of one index.
///
/// Metadata are indexed by `id` since it's the value we got on most of the endpoints.
/// The `id` column seems useless, maybe we should removed it from all the implementations?
///
/// Right now, the user is expected to have the correct tables inside it's DynamoDB instance.
/// But we could imagine creating the table on the fly with the correct indexes (right now, the indexes
/// are not complex but it could become complex in the future we the growing needs.)
///
/// TODO
/// - Documentation on table creation
/// - Try to remove clones everywhere
/// - Split ID in two columns (index_id and uid) in entries and chains?
/// - Implement sizes (right now this implementation do not know the sizes of the tables for one index)
/// - In the rare case of collusion for a `id` retry with a new one? :UniqueId
pub struct Database {
    client: Client,

    metadata_table_name: String,
    entries_table_name: String,
    chains_table_name: String,
}

const DYNAMODB_MAX_READ_ELEMENTS: usize = 100;
const DYNAMODB_MAX_WRITE_ELEMENTS: usize = 25;

/// DynomoDB doesn't provide a way to batch upsert requests,
/// but we use async to do x of them in parallel. If this value
/// is too high it can crash.
const DYNAMODB_NUMBER_OF_PARALLEL_UPSERT_REQUEST: usize = 30;
const ENTRIES_AND_CHAINS_ID_COLUMN_NAME: &str = "id";
const ENTRIES_AND_CHAINS_VALUE_COLUMN_NAME: &str = "value_bytes"; // 'value' is a reserved keyword in dynamodb

impl Database {
    pub async fn create() -> Self {
        let mut config_builder = aws_config::from_env()
            .credentials_provider(EnvironmentVariableCredentialsProvider::new())
            .retry_config(RetryConfigBuilder::new().max_attempts(10).build());

        if let Ok(url) = env::var("AWS_DYNAMODB_ENDPOINT_URL") {
            config_builder = config_builder.endpoint_url(url)
        }

        let config = config_builder.load().await;
        let client = aws_sdk_dynamodb::Client::new(&config);

        let metadata_table_name = env::var("DYNAMODB_METADATA_TABLE_NAME")
            .unwrap_or_else(|_| "findex_cloud_metadata".to_string());
        let entries_table_name = env::var("DYNAMODB_ENTRIES_TABLE_NAME")
            .unwrap_or_else(|_| "findex_cloud_entries".to_string());
        let chains_table_name = env::var("DYNAMODB_CHAINS_TABLE_NAME")
            .unwrap_or_else(|_| "findex_cloud_chains".to_string());

        try_create_table(
            client
                .create_table()
                .table_name(&metadata_table_name)
                .attribute_definitions(
                    AttributeDefinition::builder()
                        .attribute_name("id")
                        .attribute_type(ScalarAttributeType::S)
                        .build(),
                )
                .key_schema(
                    KeySchemaElement::builder()
                        .attribute_name("id")
                        .key_type(KeyType::Hash)
                        .build(),
                )
                .billing_mode(BillingMode::PayPerRequest)
                .send()
                .await,
        )
        .unwrap_or_else(|err| {
            panic!("Fail to create table {metadata_table_name} in DynamoDB ({err})")
        });

        try_create_table(
            client
                .create_table()
                .table_name(&entries_table_name)
                .attribute_definitions(
                    AttributeDefinition::builder()
                        .attribute_name(ENTRIES_AND_CHAINS_ID_COLUMN_NAME)
                        .attribute_type(ScalarAttributeType::B)
                        .build(),
                )
                .key_schema(
                    KeySchemaElement::builder()
                        .attribute_name(ENTRIES_AND_CHAINS_ID_COLUMN_NAME)
                        .key_type(KeyType::Hash)
                        .build(),
                )
                .billing_mode(BillingMode::PayPerRequest)
                .send()
                .await,
        )
        .unwrap_or_else(|err| {
            panic!("Fail to create table {entries_table_name} in DynamoDB ({err})")
        });
        try_create_table(
            client
                .create_table()
                .table_name(&chains_table_name)
                .attribute_definitions(
                    AttributeDefinition::builder()
                        .attribute_name(ENTRIES_AND_CHAINS_ID_COLUMN_NAME)
                        .attribute_type(ScalarAttributeType::B)
                        .build(),
                )
                .key_schema(
                    KeySchemaElement::builder()
                        .attribute_name(ENTRIES_AND_CHAINS_ID_COLUMN_NAME)
                        .key_type(KeyType::Hash)
                        .build(),
                )
                .billing_mode(BillingMode::PayPerRequest)
                .send()
                .await,
        )
        .unwrap_or_else(|err| {
            panic!("Fail to create table {chains_table_name} in DynamoDB ({err})")
        });

        Database {
            client,
            metadata_table_name,
            entries_table_name,
            chains_table_name,
        }
    }

    fn get_table_name(&self, table: Table) -> &str {
        match table {
            Table::Entries => &self.entries_table_name,
            Table::Chains => &self.chains_table_name,
        }
    }

    /// Fail if the uid doesn't exist
    async fn fetch_value(&self, index: &Index, table: Table, uid: &[u8]) -> Result<Vec<u8>, Error> {
        let result = self
            .client
            .get_item()
            .table_name(self.get_table_name(table))
            .key(
                ENTRIES_AND_CHAINS_ID_COLUMN_NAME,
                get_uid_attribute_value(index, uid),
            )
            .send()
            .await?;

        let item = match result.item() {
            None => {
                return Err(Error::DynamoDb(format!(
                    "Cannot find a 'value' from the key '{uid:?}"
                )))
            }
            Some(item) => item,
        };

        extract_bytes(item, ENTRIES_AND_CHAINS_VALUE_COLUMN_NAME)
    }

    async fn upsert_entry(
        &self,
        index: &Index,
        uid: Uid<UID_LENGTH>,
        old_value: Option<Vec<u8>>,
        new_value: Vec<u8>,
    ) -> Result<Option<(Uid<UID_LENGTH>, Vec<u8>)>, Error> {
        if let Some(old_value) = old_value {
            // If there is an `old_value`, we `update_item()` with a conditional
            // expression checking the previously stored value against the `old_value`.
            //
            // The value should always exists inside the database (except in case of a compact).
            // I don't know if `update_item()` fail with a specific error code if the key doesn't
            // exists (it should fail since it's a `update_item()` and not a `put_item()`).

            let result = self
                .client
                .update_item()
                .table_name(self.get_table_name(Table::Entries))
                .key(
                    ENTRIES_AND_CHAINS_ID_COLUMN_NAME,
                    get_uid_attribute_value(index, &uid),
                )
                .update_expression(format!(
                    "SET {} = :new",
                    ENTRIES_AND_CHAINS_VALUE_COLUMN_NAME
                ))
                .expression_attribute_values(
                    ":old",
                    AttributeValue::B(Blob::new(old_value.clone())),
                )
                .expression_attribute_values(
                    ":new",
                    AttributeValue::B(Blob::new(new_value.clone())),
                )
                .condition_expression(format!("{} = :old", ENTRIES_AND_CHAINS_VALUE_COLUMN_NAME))
                .send()
                .await;

            // If the conditional expression fails, we need to fetch
            // the stored value (it's impossible to return the value from an error
            // in DynamoDB) for Findex to retry with the correct `old_value`
            match result {
                Ok(_) => Ok(None),
                Err(SdkError::ServiceError(err))
                    if matches!(
                        err.err(),
                        UpdateItemError::ConditionalCheckFailedException { .. }
                    ) =>
                {
                    let value = self.fetch_value(index, Table::Entries, &uid).await?;
                    Ok(Some((uid, value)))
                }
                Err(err) => Err(Error::from(err)),
            }
        } else {
            // Here we don't have an `old_value` so we can use `put_item()`
            // with an `attribute_not_exists(id)` conditional expression to check
            // that the key doesn't already exist.

            let result = self
                .client
                .put_item()
                .table_name(self.get_table_name(Table::Entries))
                .item(
                    ENTRIES_AND_CHAINS_ID_COLUMN_NAME,
                    get_uid_attribute_value(index, &uid),
                )
                .item(
                    ENTRIES_AND_CHAINS_VALUE_COLUMN_NAME,
                    AttributeValue::B(Blob::new(new_value.clone())),
                )
                .condition_expression(format!(
                    "attribute_not_exists({})",
                    ENTRIES_AND_CHAINS_ID_COLUMN_NAME
                ))
                .send()
                .await;

            // If the conditional expression fails, we need to fetch
            // the stored value (it's impossible to return the value from an error
            // in DynamoDB) for Findex to retry with the correct `old_value`
            match result {
                Ok(_) => Ok(None),
                Err(SdkError::ServiceError(err))
                    if matches!(
                        err.err(),
                        PutItemError::ConditionalCheckFailedException { .. }
                    ) =>
                {
                    let value = self.fetch_value(index, Table::Entries, &uid).await?;

                    Ok(Some((uid, value)))
                }
                Err(err) => Err(Error::from(err)),
            }
        }
    }
}

#[async_trait]
impl IndexesDatabase for Database {
    async fn set_size(&self, _index: &mut Index) -> Result<(), Error> {
        Ok(())
    }

    async fn fetch(
        &self,
        index: &Index,
        table: Table,
        uids: HashSet<Uid<UID_LENGTH>>,
    ) -> Result<EncryptedTable<UID_LENGTH>, Error> {
        let mut uids_and_values = EncryptedTable::<UID_LENGTH>::with_capacity(uids.len());
        if uids.is_empty() {
            return Ok(uids_and_values);
        }

        let uids: Vec<_> = uids.into_iter().collect();

        for chunk in uids.chunks(DYNAMODB_MAX_READ_ELEMENTS) {
            let mut keys_and_attributes = KeysAndAttributes::builder();

            for uid in chunk {
                keys_and_attributes = keys_and_attributes.keys(HashMap::from([(
                    ENTRIES_AND_CHAINS_ID_COLUMN_NAME.to_string(),
                    get_uid_attribute_value(index, uid),
                )]));
            }
            let batch_get_item = self
                .client
                .batch_get_item()
                .request_items(self.get_table_name(table), keys_and_attributes.build());

            let results = batch_get_item.send().await?;

            if let Some(responses) = results.responses() {
                if let Some(items) = responses.get(self.get_table_name(table)) {
                    for item in items {
                        let id = extract_bytes(item, ENTRIES_AND_CHAINS_ID_COLUMN_NAME)?;
                        let uid = extract_uid_from_stored_id(id)?;

                        uids_and_values.insert(
                            uid,
                            extract_bytes(item, ENTRIES_AND_CHAINS_VALUE_COLUMN_NAME)?,
                        );
                    }
                }
            }
        }

        Ok(uids_and_values)
    }

    async fn upsert_entries(
        &self,
        index: &Index,
        data: UpsertData<UID_LENGTH>,
    ) -> Result<EncryptedTable<UID_LENGTH>, Error> {
        let mut rejected = EncryptedTable::<UID_LENGTH>::with_capacity(1);

        // This function is using a loop instead of a batch_* function
        // because DynamoDB doesn't support conditional expression on batches.
        let mut jobs =
            futures::stream::iter(data.into_iter().map(|(uid, (old_value, new_value))| {
                self.upsert_entry(index, uid, old_value, new_value)
            }))
            .buffer_unordered(DYNAMODB_NUMBER_OF_PARALLEL_UPSERT_REQUEST);

        while let Some(result) = jobs.next().await {
            if let Some((uid, value)) = result? {
                rejected.insert(uid, value);
            }
        }

        Ok(rejected)
    }

    async fn insert_chains(
        &self,
        index: &Index,
        data: EncryptedTable<UID_LENGTH>,
    ) -> Result<(), Error> {
        let data: Vec<_> = data.into_iter().collect();

        for chunk in data.chunks(DYNAMODB_MAX_WRITE_ELEMENTS) {
            self.client
                .batch_write_item()
                .request_items(
                    self.get_table_name(Table::Chains),
                    chunk
                        .iter()
                        .map(|(uid, value)| {
                            WriteRequest::builder()
                                .put_request(
                                    PutRequest::builder()
                                        .item(
                                            ENTRIES_AND_CHAINS_ID_COLUMN_NAME,
                                            get_uid_attribute_value(index, uid),
                                        )
                                        .item(
                                            ENTRIES_AND_CHAINS_VALUE_COLUMN_NAME,
                                            AttributeValue::B(Blob::new(value.clone())),
                                        )
                                        .build(),
                                )
                                .build()
                        })
                        .collect(),
                )
                .send()
                .await?;
        }

        Ok(())
    }

    #[cfg(feature = "log_requests")]
    async fn fetch_all_as_json(&self, _index: &Index, _table: Table) -> Result<String, Error> {
        unimplemented!();
    }
}

#[async_trait]
impl MetadataDatabase for Database {
    async fn get_indexes(&self) -> Result<Vec<Index>, Error> {
        let response = self
            .client
            .scan()
            .table_name(&self.metadata_table_name)
            .send()
            .await?;

        match response.items() {
            None => Ok(vec![]), // Don't know why this function return an option
            Some(items) => Ok(items
                .iter()
                .map(item_to_index)
                .collect::<Result<Vec<_>, _>>()?),
        }
    }

    async fn get_index(&self, id: &str) -> Result<Option<Index>, Error> {
        let item = self
            .client
            .get_item()
            .table_name(&self.metadata_table_name)
            .key("id", AttributeValue::S(id.to_string()))
            .send()
            .await?;

        match item.item() {
            None => Ok(None),
            Some(item) => Ok(Some(item_to_index(item)?)),
        }
    }

    async fn delete_index(&self, id: &str) -> Result<(), Error> {
        self.client
            .delete_item()
            .key("id", AttributeValue::S(id.to_string()))
            .send()
            .await?;

        Ok(())
    }

    async fn create_index(&self, new_index: NewIndex) -> Result<Index, Error> {
        let index = Index {
            id: new_index.id,
            name: new_index.name,
            fetch_entries_key: new_index.fetch_entries_key,
            fetch_chains_key: new_index.fetch_chains_key,
            upsert_entries_key: new_index.upsert_entries_key,
            insert_chains_key: new_index.insert_chains_key,
            size: Some(0),
            created_at: Utc::now().naive_utc(),
        };

        // This will override the previous index if the `id` is not unique
        // :UniqueId
        self.client
            .put_item()
            .table_name(&self.metadata_table_name)
            .item("id", AttributeValue::S(index.id.clone()))
            .item("name", AttributeValue::S(index.name.clone()))
            .item(
                "fetch_entries_key",
                AttributeValue::B(Blob::new(index.fetch_entries_key.clone())),
            )
            .item(
                "fetch_chains_key",
                AttributeValue::B(Blob::new(index.fetch_chains_key.clone())),
            )
            .item(
                "upsert_entries_key",
                AttributeValue::B(Blob::new(index.upsert_entries_key.clone())),
            )
            .item(
                "insert_chains_key",
                AttributeValue::B(Blob::new(index.insert_chains_key.clone())),
            )
            .item(
                "created_at",
                AttributeValue::S(index.created_at.to_string()),
            )
            .send()
            .await?;

        Ok(index)
    }
}

/// Create the ID to store inside DynamoDB from Index `id` and `uid`
/// This function is the inverse of `extract_uid_from_stored_id`.
fn get_uid_attribute_value(index: &Index, uid: &[u8]) -> AttributeValue {
    let index_id_bytes = index.id.as_bytes();

    let mut id = Vec::with_capacity(index_id_bytes.len() + uid.len());
    id.extend_from_slice(index_id_bytes);
    id.extend_from_slice(uid);

    AttributeValue::B(Blob::new(id))
}

/// Extract the `uid` from the ID stored inside DynamoDB
/// This function is the inverse of `get_uid_attribute_value`.
fn extract_uid_from_stored_id(id: Vec<u8>) -> Result<Uid<UID_LENGTH>, Error> {
    let uid: [u8; UID_LENGTH] =
        id.as_slice()[id.len() - UID_LENGTH..]
            .try_into()
            .map_err(|_| {
                Error::DynamoDb(format!(
                    "Cannot find the UID at the tail of the ID stored inside DynamoDB '{id:?}'"
                ))
            })?;

    Ok(Uid::from(uid))
}

fn extract_bytes(item: &HashMap<String, AttributeValue>, key: &str) -> Result<Vec<u8>, Error> {
    Ok(item
        .get(key)
        .ok_or_else(|| Error::DynamoDb(format!("{item:?} doesn't contains an '{key}' attribute.")))?
        .as_b()
        .map_err(|_| {
            Error::DynamoDb(format!(
                "{item:?} contains a '{key}' attribute but it's not bytes."
            ))
        })?
        .clone()
        .into_inner())
}

fn extract_string(item: &HashMap<String, AttributeValue>, key: &str) -> Result<String, Error> {
    Ok(item
        .get(key)
        .ok_or_else(|| Error::DynamoDb(format!("{item:?} doesn't contains an '{key}' attribute.")))?
        .as_s()
        .map_err(|_| {
            Error::DynamoDb(format!(
                "{item:?} contains a '{key}' attribute but it's not a 'string'."
            ))
        })?
        .clone())
}

fn try_create_table(
    response: Result<CreateTableOutput, SdkError<CreateTableError>>,
) -> Result<(), SdkError<CreateTableError>> {
    match response {
        Ok(_) => Ok(()),
        Err(SdkError::ServiceError(err))
            if matches!(err.err(), CreateTableError::ResourceInUseException(_)) =>
        {
            Ok(())
        }
        Err(err) => Err(err),
    }
}

fn item_to_index(item: &HashMap<String, AttributeValue>) -> Result<Index, Error> {
    let created_at = extract_string(item, "created_at")?;

    Ok(Index {
        id: extract_string(item, "id")?,
        name: extract_string(item, "name")?,
        fetch_entries_key: extract_bytes(item, "fetch_entries_key")?,
        fetch_chains_key: extract_bytes(item, "fetch_chains_key")?,
        upsert_entries_key: extract_bytes(item, "upsert_entries_key")?,
        insert_chains_key: extract_bytes(item, "insert_chains_key")?,
        size: None,
        created_at: NaiveDateTime::parse_from_str(&created_at, "%Y-%m-%d %H:%M:%S%.f").map_err(
            |_| {
                Error::DynamoDb(format!(
                    "Cannot parse date '{created_at}' inside 'created_at' attribute."
                ))
            },
        )?,
    })
}
