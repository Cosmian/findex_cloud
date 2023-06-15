#[cfg(feature = "multitenant")]
use std::env;

use std::{
    collections::{HashMap, HashSet},
    future::Future,
    pin::Pin,
    sync::RwLock,
    time::SystemTime,
};

use actix_web::{
    dev::Payload,
    web::{Bytes, Data, Path},
    FromRequest,
};
use async_trait::async_trait;
use cloudproof_findex::cloud::{CALLBACK_SIGNATURE_LENGTH, SIGNATURE_SEED_LENGTH};

use chrono::NaiveDateTime;
use cosmian_crypto_core::bytes_ser_de::Serializable;
use cosmian_findex::{
    kmac,
    parameters::{KmacKey, UID_LENGTH},
    EncryptedTable, KeyingMaterial, Uid, UpsertData,
};
use serde::{Deserialize, Serialize};

#[cfg(feature = "multitenant")]
use crate::auth0::Auth;

use crate::errors::Error;

#[derive(Serialize, Debug, Clone)]
pub(crate) struct Index {
    #[serde(skip_serializing)]
    pub(crate) id: i64,
    pub(crate) public_id: String,
    pub(crate) authz_id: String,
    pub(crate) project_uuid: String,
    pub(crate) name: String,
    pub(crate) fetch_entries_key: Vec<u8>,
    pub(crate) fetch_chains_key: Vec<u8>,
    pub(crate) upsert_entries_key: Vec<u8>,
    pub(crate) insert_chains_key: Vec<u8>,
    pub(crate) size: Option<i64>,
    pub(crate) created_at: NaiveDateTime,
    #[serde(skip_serializing)]
    #[allow(dead_code)]
    pub(crate) deleted_at: Option<NaiveDateTime>,
}

#[derive(Debug)]
pub(crate) struct NewIndex {
    pub(crate) public_id: String,
    pub(crate) authz_id: String,
    pub(crate) project_uuid: String,
    pub(crate) name: String,
    pub(crate) fetch_entries_key: Vec<u8>,
    pub(crate) fetch_chains_key: Vec<u8>,
    pub(crate) upsert_entries_key: Vec<u8>,
    pub(crate) insert_chains_key: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct UidAndValue {
    pub(crate) uid: String,
    pub(crate) value: String,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct UidAndOldAndNewValues {
    pub(crate) uid: String,
    pub(crate) old_value: Option<String>,
    pub(crate) new_value: String,
}

#[allow(clippy::result_large_err)]
pub(crate) fn check_body_signature(
    body: Bytes,
    index_id: &str,
    seed: &[u8],
) -> Result<Vec<u8>, Error> {
    let original_length = body.len();
    let mut bytes = body.into_iter();

    let signature_received = bytes
        .next_chunk::<CALLBACK_SIGNATURE_LENGTH>()
        .map_err(|_| {
            Error::BadRequest(format!(
                "Body of request is too small ({original_length} bytes), not enought bytes to read signature.",
            ))
        })?;

    let timestamp_bytes = bytes
        .next_chunk()
        .map_err(|_| Error::BadRequest(format!("Body of request is too small ({original_length} bytes), not enought bytes to read expiration timestamp.")))?;

    let data: Vec<_> = bytes.collect();

    let key: KmacKey =
        KeyingMaterial::<SIGNATURE_SEED_LENGTH>::try_from_bytes(seed.to_vec().as_slice())
            .unwrap()
            .derive_kmac_key(index_id.as_bytes());

    let signature_computed = kmac!(CALLBACK_SIGNATURE_LENGTH, &key, &timestamp_bytes, &data);

    if signature_received != signature_computed {
        return Err(Error::InvalidSignature);
    }

    let expiration_timestamp = u64::from_be_bytes(timestamp_bytes);
    let current_timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|_| Error::BadRequest("SystemTime is before UNIX_EPOCH".to_owned()))?
        .as_secs();

    if current_timestamp > expiration_timestamp {
        return Err(Error::BadRequest(format!("Request expired (current time is {current_timestamp}, expiration time is {expiration_timestamp})")));
    }

    Ok(data)
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum Table {
    Entries,
    Chains,
}

#[async_trait]
pub(crate) trait IndexesDatabase: Sync + Send {
    async fn set_sizes(&self, indexes: &mut Vec<Index>) -> Result<(), Error> {
        for index in indexes {
            self.set_size(index).await?;
        }

        Ok(())
    }

    async fn set_size(&self, indexes: &mut Index) -> Result<(), Error>;

    async fn fetch(
        &self,
        index: &Index,
        table: Table,
        uids: HashSet<Uid<UID_LENGTH>>,
    ) -> Result<EncryptedTable<UID_LENGTH>, Error>;

    async fn upsert_entries(
        &self,
        index: &Index,
        data: UpsertData<UID_LENGTH>,
    ) -> Result<EncryptedTable<UID_LENGTH>, Error>;

    async fn insert_chains(
        &self,
        index: &Index,
        data: EncryptedTable<UID_LENGTH>,
    ) -> Result<(), Error>;

    #[cfg(feature = "log_requests")]
    async fn fetch_all_as_json(&self, index: &Index, table: Table) -> Result<String, Error>;
}

pub(crate) type MetadataCache = RwLock<HashMap<String, Index>>;

#[async_trait]
pub(crate) trait MetadataDatabase: Sync + Send {
    async fn get_indexes(&self, project_uuid: &str) -> Result<Vec<Index>, Error>;

    async fn get_index(&self, public_id: &str) -> Result<Option<Index>, Error>;
    async fn get_index_with_cache(
        &self,
        cache: &MetadataCache,
        public_id: &str,
    ) -> Result<Option<Index>, Error> {
        if let Ok(cache) = cache.read() {
            if let Some(index) = cache.get(public_id) {
                return Ok(Some(index.clone()));
            }
        }

        let index = self.get_index(public_id).await?;

        if let Some(index) = index {
            if let Ok(mut cache) = cache.write() {
                cache.insert(public_id.to_string(), index.clone());
            }

            return Ok(Some(index));
        }

        return Ok(None);
    }

    async fn delete_index(&self, public_id: &str) -> Result<(), Error>;
    async fn create_index(&self, new_index: NewIndex) -> Result<Index, Error>;
}

impl FromRequest for Index {
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>;

    fn from_request(req: &actix_web::HttpRequest, _: &mut Payload) -> Self::Future {
        let req = req.clone();

        Box::pin(async move {
            let metadata_cache = req.app_data::<Data<MetadataCache>>().unwrap();
            let metadata_database = req.app_data::<Data<dyn MetadataDatabase>>().unwrap();

            let public_id: Path<String> = Path::<String>::extract(&req)
                .await
                .map_err(|_| Error::WrongIndexPublicId)?;

            let index = metadata_database
                .get_index_with_cache(&metadata_cache, &public_id)
                .await?;

            if let Some(index) = index {
                Ok(index)
            } else {
                Err(Error::BadRequest(format!(
                    "Unknown index for ID {public_id}"
                )))
            }
        })
    }
}

#[cfg(feature = "multitenant")]
pub(crate) struct Backend {
    pub(crate) domain: String,
}

#[cfg(feature = "multitenant")]
impl Backend {
    pub(crate) fn from_env() -> Self {
        Self {
            domain: env::var("BACKEND_DOMAIN").expect(
                "Please set the `BACKEND_DOMAIN` environment variable. Example: \
                \"backend.mse.cosmian.com\"",
            ),
        }
    }
}

#[cfg(feature = "multitenant")]
#[derive(Debug, Deserialize, PartialEq)]
pub(crate) struct BackendProject {
    pub(crate) id: String,
}

#[cfg(feature = "multitenant")]
impl BackendProject {
    pub(crate) async fn get_projects(backend: &Backend, auth: &Auth) -> Result<Vec<Self>, Error> {
        Ok(reqwest::Client::new()
            .get(&format!("https://{}/projects", backend.domain))
            .bearer_auth(&auth.bearer)
            .send()
            .await?
            .json()
            .await?)
    }
}
