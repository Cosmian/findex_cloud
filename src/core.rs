use std::{env, future::Future, pin::Pin};

use actix_web::{
    dev::Payload,
    web::{Bytes, Data, Path},
    FromRequest, HttpRequest,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tiny_keccak::{Hasher, Kmac};

use crate::{auth0::Auth, errors::Error};

pub(crate) struct Id {
    pub(crate) id: i64,
}

#[derive(Serialize)]
pub(crate) struct Index {
    pub(crate) id: i64,
    pub(crate) authz_id: String,
    pub(crate) project_uuid: String,
    pub(crate) public_id: String,
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

pub(crate) fn check_body_signature(
    request: &HttpRequest,
    bytes: &Bytes,
    key: &[u8],
) -> Result<(), Error> {
    let mut hasher = Kmac::v128(key, &[]);
    let mut output = [0u8; 32];
    hasher.update(bytes);
    hasher.finalize(&mut output);

    if hex::encode(output)
        != request
            .headers()
            .get("x-findex-cloud-signature")
            .and_then(|header| header.to_str().ok())
            .unwrap_or_default()
    {
        return Err(Error::InvalidSignature);
    }

    Ok(())
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
                r#"SELECT * FROM indexes WHERE public_id = $1"#,
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
