use std::{
    collections::{HashMap, HashSet},
    env,
};

use async_trait::async_trait;
use aws_config::environment::EnvironmentVariableCredentialsProvider;
use aws_sdk_dynamodb::{
    operation::{put_item::PutItemError, update_item::UpdateItemError},
    primitives::Blob,
    types::{AttributeValue, KeysAndAttributes, PutRequest, WriteRequest},
    Client,
};
use aws_smithy_http::result::SdkError;
use chrono::{NaiveDateTime, Utc};
use cosmian_findex::{parameters::UID_LENGTH, EncryptedTable, Uid, UpsertData};

use crate::{
    core::{Index, IndexesDatabase, MetadataDatabase, NewIndex, Table},
    errors::Error,
};

pub struct Database {
    client: Client,

    metadata_table_name: String,
    entries_table_name: String,
    chains_table_name: String,
}

const ENTRIES_AND_CHAINS_ID_COLUMN_NAME: &str = "id";
const ENTRIES_AND_CHAINS_VALUE_COLUMN_NAME: &str = "value_bytes"; // 'value' is a reserved keyword in dynamodb

impl Database {
    pub async fn create() -> Self {
        let mut config_builder = aws_config::from_env()
            .credentials_provider(EnvironmentVariableCredentialsProvider::new());

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

        let mut keys_and_attributes = KeysAndAttributes::builder();

        for uid in uids.into_iter() {
            keys_and_attributes = keys_and_attributes.keys(HashMap::from([(
                ENTRIES_AND_CHAINS_ID_COLUMN_NAME.to_string(),
                get_uid_attribute_value(index, &uid),
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

        Ok(uids_and_values)
    }

    async fn upsert_entries(
        &self,
        index: &Index,
        data: UpsertData<UID_LENGTH>,
    ) -> Result<EncryptedTable<UID_LENGTH>, Error> {
        let mut rejected = EncryptedTable::<UID_LENGTH>::with_capacity(1);

        let data: Vec<_> = data.into_iter().collect();

        for (uid, (old_value, new_value)) in data {
            if let Some(old_value) = old_value {
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
                    .condition_expression(format!(
                        "{} = :old",
                        ENTRIES_AND_CHAINS_VALUE_COLUMN_NAME
                    ))
                    .send()
                    .await;

                match result {
                    Ok(_) => {}
                    Err(SdkError::ServiceError(err))
                        if matches!(
                            err.err(),
                            UpdateItemError::ConditionalCheckFailedException { .. }
                        ) =>
                    {
                        let value = self.fetch_value(index, Table::Entries, &uid).await?;
                        rejected.insert(uid, value);
                    }
                    Err(err) => {
                        dbg!(&err);
                        return Err(Error::from(err));
                    }
                }
            } else {
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
                    )) // only if it doesn't exists
                    .send()
                    .await;

                match result {
                    Ok(_) => {}
                    Err(SdkError::ServiceError(err))
                        if matches!(
                            err.err(),
                            PutItemError::ConditionalCheckFailedException { .. }
                        ) =>
                    {
                        let value = self.fetch_value(index, Table::Entries, &uid).await?;
                        rejected.insert(uid, value);
                    }
                    Err(err) => {
                        dbg!(&err);
                        return Err(Error::from(err));
                    }
                }
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

        for chunk in data.chunks(25) {
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
        todo!();
    }
}

#[async_trait]
impl MetadataDatabase for Database {
    async fn get_indexes(&self, _project_uuid: &str) -> Result<Vec<Index>, Error> {
        Ok(vec![])
    }

    async fn get_index(&self, public_id: &str) -> Result<Option<Index>, Error> {
        let item = self
            .client
            .get_item()
            .table_name(&self.metadata_table_name)
            .key("public_id", AttributeValue::S(public_id.to_string()))
            .send()
            .await?;

        let index = match item.item() {
            None => return Ok(None),
            Some(item) => {
                let created_at = extract_string(item, "created_at")?;

                Index {
                    id: extract_number(item, "id")?,
                    public_id: extract_string(item, "public_id")?,
                    authz_id: extract_string(item, "authz_id")?,
                    project_uuid: extract_string(item, "project_uuid")?,
                    name: extract_string(item, "name")?,
                    fetch_entries_key: extract_bytes(item, "fetch_entries_key")?,
                    fetch_chains_key: extract_bytes(item, "fetch_chains_key")?,
                    upsert_entries_key: extract_bytes(item, "upsert_entries_key")?,
                    insert_chains_key: extract_bytes(item, "insert_chains_key")?,
                    size: None,
                    created_at: NaiveDateTime::parse_from_str(&created_at, "%Y-%m-%d %H:%M:%S%.f")
                        .map_err(|_| {
                            Error::DynamoDb(format!(
                                "Cannot parse date '{created_at}' inside 'created_at' attribute."
                            ))
                        })?,
                    deleted_at: None,
                }
            }
        };

        Ok(Some(index))
    }

    async fn delete_index(&self, public_id: &str) -> Result<(), Error> {
        self.client
            .delete_item()
            .key("public_id", AttributeValue::S(public_id.to_string()))
            .send()
            .await?;

        Ok(())
    }

    async fn create_index(&self, new_index: NewIndex) -> Result<Index, Error> {
        let index = Index {
            id: 42,
            public_id: new_index.public_id,
            authz_id: new_index.authz_id,
            project_uuid: new_index.project_uuid,
            name: new_index.name,
            fetch_entries_key: new_index.fetch_entries_key,
            fetch_chains_key: new_index.fetch_chains_key,
            upsert_entries_key: new_index.upsert_entries_key,
            insert_chains_key: new_index.insert_chains_key,
            size: Some(0),
            created_at: Utc::now().naive_utc(),
            deleted_at: None,
        };

        self.client
            .put_item()
            .table_name("metadata")
            .item("id", AttributeValue::N(index.id.to_string()))
            .item("public_id", AttributeValue::S(index.public_id.clone()))
            .item("authz_id", AttributeValue::S(index.authz_id.clone()))
            .item(
                "project_uuid",
                AttributeValue::S(index.project_uuid.clone()),
            )
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

fn get_uid_attribute_value(index: &Index, uid: &[u8]) -> AttributeValue {
    let public_id_bytes = index.public_id.as_bytes();

    let mut id = Vec::with_capacity(public_id_bytes.len() + uid.len());
    id.extend_from_slice(public_id_bytes);
    id.extend_from_slice(uid);

    AttributeValue::B(Blob::new(id))
}

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

fn extract_number(item: &HashMap<String, AttributeValue>, key: &str) -> Result<i64, Error> {
    item.get(key)
        .ok_or_else(|| Error::DynamoDb(format!("{item:?} doesn't contains an '{key}' attribute.")))?
        .as_n()
        .map_err(|_| {
            Error::DynamoDb(format!(
                "{item:?} contains a '{key}' attribute but it's not a 'number'."
            ))
        })?
        .parse()
        .map_err(|_| {
            Error::DynamoDb(format!(
                "{item:?} contains a '{key}' attribute but cannot parse it as a 'number'."
            ))
        })
}
