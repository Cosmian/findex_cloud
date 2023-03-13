use std::{env, future::Future, pin::Pin, time::SystemTime};

use actix_web::{
    dev::Payload,
    web::{Bytes, Data, Path},
    FromRequest,
};
use cloudproof_findex::cloud::{CALLBACK_SIGNATURE_LENGTH, SIGNATURE_SEED_LENGTH};

use cosmian_crypto_core::bytes_ser_de::Serializable;
use cosmian_findex::{kmac, parameters::KmacKey, KeyingMaterial};
use serde::{Deserialize, Serialize};
use sqlx::{types::chrono::NaiveDateTime, SqlitePool};

use crate::{auth0::Auth, errors::Error};

pub(crate) struct Id {
    pub(crate) id: i64,
}

#[derive(Serialize)]
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
    dbg!(
        original_length,
        timestamp_bytes,
        signature_received,
        signature_computed,
        key,
    );
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

impl FromRequest for Index {
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>;

    fn from_request(req: &actix_web::HttpRequest, _: &mut Payload) -> Self::Future {
        let req = req.clone();

        Box::pin(async move {
            let pool = req.app_data::<Data<SqlitePool>>().unwrap();
            let mut db = pool.acquire().await?;

            let public_id: Path<String> = Path::<String>::extract(&req)
                .await
                .map_err(|_| Error::WrongIndexPublicId)?;

            Ok(sqlx::query_as!(
                Index,
                r#"SELECT *, null as "size: _" FROM indexes WHERE public_id = $1 AND deleted_at IS NULL"#,
                *public_id
            )
            .fetch_one(&mut db)
            .await?)
        })
    }
}

pub(crate) struct Backend {
    pub(crate) domain: String,
}

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

#[derive(Debug, Deserialize, PartialEq)]
pub(crate) struct BackendProject {
    pub(crate) uuid: String,
}

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
