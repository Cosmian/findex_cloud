#![feature(iter_next_chunk)]

#[cfg(feature = "log_requests")]
use crate::debug_logs::DataTimeDiffInMillisecondsMutex;

use std::env;
use std::sync::Arc;

#[cfg(feature = "multitenant")]
use crate::auth0::{Auth, Auth0};
#[cfg(feature = "multitenant")]
use crate::core::{Backend, BackendProject};
use crate::core::{IndexesDatabase, Table};
use crate::errors::Error;
use actix_web::web::PayloadConfig;
#[cfg(feature = "multitenant")]
use actix_web::web::Query;
use sqlx::migrate::MigrateDatabase;
use sqlx::Sqlite;

use crate::{
    core::{check_body_signature, Id, Index},
    errors::{Response, ResponseBytes},
};
use actix_cors::Cors;
use actix_files as fs;
use actix_web::{
    delete, get,
    middleware::Logger,
    post,
    web::{Bytes, Data, Json, Path},
    App, HttpResponse, HttpServer,
};
use cloudproof_findex::ser_de::deserialize_set;
use cosmian_crypto_core::bytes_ser_de::Serializable;
use cosmian_crypto_core::CsRng;
use cosmian_findex::{parameters::UID_LENGTH, CoreError, EncryptedTable, Uid, UpsertData};
use env_logger::Env;
use rand::{distributions::Alphanumeric, Rng, RngCore, SeedableRng};
use serde::Deserialize;
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};
use std::path::Path as FsPath;

#[cfg(feature = "multitenant")]
mod auth0;
mod core;
#[cfg(feature = "log_requests")]
mod debug_logs;
mod errors;

#[cfg(feature = "heed")]
mod heed;

#[cfg(feature = "rocksdb")]
mod rocksdb;

#[cfg(not(feature = "multitenant"))]
const SINGLE_TENANT_PROJECT_UUID: &str = "SINGLE_TENANT_PROJECT_UUID";

#[cfg(not(feature = "multitenant"))]
const SINGLE_TENANT_AUTHZ_ID: &str = "SINGLE_TENANT_AUTHZ_ID";

#[cfg(feature = "multitenant")]
#[derive(Deserialize)]
struct GetIndexQuery {
    project_uuid: String,
}

#[get("/indexes")]
async fn get_indexes(
    pool: Data<SqlitePool>,
    #[cfg(feature = "multitenant")] backend: Data<Backend>,
    #[cfg(feature = "multitenant")] auth: Auth,
    #[cfg(feature = "multitenant")] params: Query<GetIndexQuery>,
    indexes_db: Data<dyn IndexesDatabase>,
) -> Response<Vec<Index>> {
    #[cfg(feature = "multitenant")]
    {
        let projects = BackendProject::get_projects(&backend, &auth).await?;

        if !projects.contains(&BackendProject {
            uuid: params.project_uuid.clone(),
        }) {
            return Err(Error::UnknownProject(params.project_uuid.clone()));
        }
    }

    let mut db = pool.acquire().await?;

    #[cfg(not(feature = "multitenant"))]
    let project_uuid = SINGLE_TENANT_PROJECT_UUID;
    #[cfg(feature = "multitenant")]
    let project_uuid = &params.project_uuid;

    let mut indexes = sqlx::query_as!(
        Index,
        r#"
            SELECT
                *,
                null as "size: _"
            FROM indexes
            WHERE project_uuid = $1 AND deleted_at IS NULL
            ORDER BY created_at DESC"#,
        project_uuid,
    )
    .fetch_all(&mut db)
    .await?;

    indexes_db.set_sizes(&mut indexes)?;

    Ok(Json(indexes))
}

#[derive(Deserialize)]
struct NewIndex {
    #[cfg(feature = "multitenant")]
    project_uuid: String,
    name: String,
}

#[post("/indexes")]
async fn post_indexes(
    pool: Data<SqlitePool>,
    #[cfg(feature = "multitenant")] backend: Data<Backend>,
    #[cfg(feature = "multitenant")] auth: Auth,
    body: Json<NewIndex>,
) -> Response<Index> {
    #[cfg(feature = "multitenant")]
    {
        let projects = BackendProject::get_projects(&backend, &auth).await?;

        if !projects.contains(&BackendProject {
            uuid: body.project_uuid.clone(),
        }) {
            return Err(Error::UnknownProject(body.project_uuid.clone()));
        }
    }

    let mut db = pool.acquire().await?;
    let mut rng = CsRng::from_entropy();

    let mut fetch_entries_key = vec![0; 16];
    rng.fill_bytes(&mut fetch_entries_key);
    let mut fetch_chains_key = vec![0; 16];
    rng.fill_bytes(&mut fetch_chains_key);
    let mut upsert_entries_key = vec![0; 16];
    rng.fill_bytes(&mut upsert_entries_key);
    let mut insert_chains_key = vec![0; 16];
    rng.fill_bytes(&mut insert_chains_key);

    let public_id: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(5)
        .map(char::from)
        .collect();

    #[cfg(not(feature = "multitenant"))]
    let authz_id = SINGLE_TENANT_AUTHZ_ID;
    #[cfg(feature = "multitenant")]
    let authz_id = auth.authz_id;

    #[cfg(not(feature = "multitenant"))]
    let project_uuid = SINGLE_TENANT_PROJECT_UUID;
    #[cfg(feature = "multitenant")]
    let project_uuid = &body.project_uuid;

    let Id { id } = sqlx::query_as!(
        Id,
        r#"INSERT INTO indexes (
            public_id,

            authz_id,
            project_uuid,

            name,

            fetch_entries_key,
            fetch_chains_key,
            upsert_entries_key,
            insert_chains_key
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id"#,
        public_id,
        authz_id,
        project_uuid,
        body.name,
        fetch_entries_key,
        fetch_chains_key,
        upsert_entries_key,
        insert_chains_key,
    )
    .fetch_one(&mut db)
    .await?;

    let index = sqlx::query_as!(
        Index,
        r#"SELECT *, null as "size: _" FROM indexes WHERE id = $1"#,
        id
    )
    .fetch_one(&mut db)
    .await?;

    Ok(Json(index))
}

#[get("/indexes/{public_id}")]
async fn get_index(
    pool: Data<SqlitePool>,
    #[cfg(feature = "multitenant")] auth: Auth,
    public_id: Path<String>,
    indexes_db: Data<dyn IndexesDatabase>,
) -> Response<Index> {
    let mut db = pool.acquire().await?;

    #[cfg(not(feature = "multitenant"))]
    let authz_id = SINGLE_TENANT_AUTHZ_ID;
    #[cfg(feature = "multitenant")]
    let authz_id = auth.authz_id;

    let index = sqlx::query_as!(
        Index,
        r#"
            SELECT
                *,
                null as "size: _"
            FROM indexes
            WHERE public_id = $1 AND authz_id = $2 AND deleted_at IS NULL
        "#,
        *public_id,
        authz_id,
    )
    .fetch_optional(&mut db)
    .await?;

    if let Some(mut index) = index {
        indexes_db.set_size(&mut index)?;
        Ok(Json(index))
    } else {
        Err(Error::BadRequest(format!(
            "Unknown index for ID {public_id}"
        )))
    }
}

#[delete("/indexes/{public_id}")]
async fn delete_index(
    pool: Data<SqlitePool>,
    #[cfg(feature = "multitenant")] auth: Auth,
    public_id: Path<String>,
) -> Response<()> {
    let mut db = pool.acquire().await?;

    #[cfg(not(feature = "multitenant"))]
    let authz_id = SINGLE_TENANT_AUTHZ_ID;
    #[cfg(feature = "multitenant")]
    let authz_id = auth.authz_id;

    sqlx::query_as!(
        Index,
        r#"
            UPDATE indexes
            SET deleted_at = current_timestamp
            WHERE public_id = $1 AND authz_id = $2
        "#,
        *public_id,
        authz_id,
    )
    .execute(&mut db)
    .await?;

    Ok(Json(()))
}

#[post("/indexes/{public_id}/fetch_entries")]
async fn fetch_entries(
    index: Index,
    bytes: Bytes,
    indexes: Data<dyn IndexesDatabase>,
    #[cfg(feature = "log_requests")] time_diff_mutex: DataTimeDiffInMillisecondsMutex,
) -> ResponseBytes {
    let bytes = check_body_signature(bytes, &index.public_id, &index.fetch_entries_key)?;
    let uids = deserialize_set::<CoreError, Uid<UID_LENGTH>>(&bytes)?;

    #[cfg(feature = "log_requests")]
    let cloned_uids = uids.clone();

    let uids_and_values = indexes.fetch(&index, Table::Entries, uids)?;

    #[cfg(feature = "log_requests")]
    crate::debug_logs::save_log(
        "fetch_entries",
        time_diff_mutex,
        cloned_uids,
        &uids_and_values,
    );

    Ok(HttpResponse::Ok()
        .content_type("application/octet-stream")
        .body(uids_and_values.try_to_bytes()?))
}

#[post("/indexes/{public_id}/fetch_chains")]
async fn fetch_chains(
    index: Index,
    bytes: Bytes,
    indexes: Data<dyn IndexesDatabase>,
    #[cfg(feature = "log_requests")] time_diff_mutex: DataTimeDiffInMillisecondsMutex,
) -> ResponseBytes {
    let bytes = check_body_signature(bytes, &index.public_id, &index.fetch_chains_key)?;
    let uids = deserialize_set::<CoreError, Uid<UID_LENGTH>>(&bytes)?;

    #[cfg(feature = "log_requests")]
    let cloned_uids = uids.clone();

    let uids_and_values = indexes.fetch(&index, Table::Chains, uids)?;

    #[cfg(feature = "log_requests")]
    crate::debug_logs::save_log(
        "fetch_chains",
        time_diff_mutex,
        cloned_uids,
        &uids_and_values,
    );

    Ok(HttpResponse::Ok()
        .content_type("application/octet-stream")
        .body(uids_and_values.try_to_bytes()?))
}

#[post("/indexes/{public_id}/upsert_entries")]
async fn upsert_entries(
    bytes: Bytes,
    index: Index,
    indexes: Data<dyn IndexesDatabase>,
) -> ResponseBytes {
    let bytes = check_body_signature(bytes, &index.public_id, &index.upsert_entries_key)?;
    let data = UpsertData::<UID_LENGTH>::try_from_bytes(&bytes)?;

    let rejected = indexes.upsert_entries(&index, data)?;

    Ok(HttpResponse::Ok()
        .content_type("application/octet-stream")
        .body(rejected.try_to_bytes()?))
}

#[post("/indexes/{public_id}/insert_chains")]
async fn insert_chains(
    index: Index,
    bytes: Bytes,
    indexes: Data<dyn IndexesDatabase>,
) -> Response<()> {
    let bytes = check_body_signature(bytes, &index.public_id, &index.insert_chains_key)?;
    let data = EncryptedTable::<UID_LENGTH>::try_from_bytes(&bytes)?;

    indexes.insert_chains(&index, data)?;

    Ok(Json(()))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    if FsPath::new(".env").exists() {
        dotenv::dotenv().expect("Cannot load env");
    }

    env_logger::Builder::from_env(Env::default().default_filter_or("debug")).init();

    let db_url = "sqlite://data/database.sqlite";

    if !Sqlite::database_exists(db_url)
        .await
        .unwrap_or_else(|e| panic!("Cannot check database existance at {db_url} ({e})"))
    {
        Sqlite::create_database(db_url)
            .await
            .unwrap_or_else(|e| panic!("Cannot create database {db_url} ({e})"));
    }
    let pool = SqlitePoolOptions::new()
        .connect(db_url)
        .await
        .unwrap_or_else(|e| panic!("Cannot connect to database at {db_url} ({e})"));

    sqlx::migrate!()
        .run(&pool)
        .await
        .unwrap_or_else(|e| panic!("Cannot run migration on database at {db_url} ({e})"));

    // Save a cloned pool before async move `pool` inside the task::spawn.
    let pool_cloned = pool.clone();

    match start_server(pool_cloned.clone(), true).await {
        Ok(_) => Ok(()),
        Err(_) => start_server(pool_cloned, false).await,
    }
}

async fn start_server(pool: SqlitePool, ipv6: bool) -> std::io::Result<()> {
    #[cfg(feature = "multitenant")]
    let auth0 = Data::new(Auth0::from_env());

    #[cfg(feature = "multitenant")]
    let backend = Data::new(Backend::from_env());

    let database_pool = Data::new(pool);

    let indexes_database: Data<dyn IndexesDatabase> = match env::var("INDEXES_DATABASE_TYPE").as_deref().unwrap_or("rocksdb") {
            #[cfg(feature = "heed")]
            "heed" => Data::from(Arc::new(crate::heed::Database::create()) as Arc<dyn IndexesDatabase>),
            #[cfg(not(feature = "heed"))]
            "heed" => panic!("Cannot load `INDEXES_DATABASE_TYPE=heed` because `findex_cloud` wasn't compiled with \"heed\" feature."),
            
            #[cfg(feature = "rocksdb")]
            "rocksdb" => Data::from(Arc::new(crate::rocksdb::Database::create()) as Arc<dyn IndexesDatabase>),
            #[cfg(not(feature = "rocksdb"))]
            "rocksdb" => panic!("Cannot load `INDEXES_DATABASE_TYPE=rocksdb` because `findex_cloud` wasn't compiled with \"rocksdb\" feature."),

            indexes_database_type => panic!("Unknown `INDEXES_DATABASE_TYPE` env variable `{indexes_database_type}` (please use `rocksdb` or `heed`)"),
        };

    #[cfg(feature = "log_requests")]
    let time_mock: DataTimeDiffInMillisecondsMutex = Data::new(Default::default());

    let mut server = HttpServer::new(move || {
        #[allow(unused_mut)]
        let mut app = App::new()
            .wrap(Cors::permissive())
            .wrap(Logger::default())
            .app_data(database_pool.clone())
            .app_data(indexes_database.clone())
            .app_data(PayloadConfig::new(50_000_000))
            .service(get_index)
            .service(get_indexes)
            .service(post_indexes)
            .service(delete_index)
            .service(fetch_entries)
            .service(fetch_chains)
            .service(upsert_entries)
            .service(insert_chains);

        #[cfg(feature = "multitenant")]
        {
            app = app.app_data(auth0.clone());
            app = app.app_data(backend.clone());
        }

        #[cfg(feature = "log_requests")]
        {
            app = app
                .app_data(time_mock.clone())
                .service(crate::debug_logs::set_time_diff)
                .service(crate::debug_logs::post_reset_requests_log)
                .service(crate::debug_logs::get_requests_log)
                .service(crate::debug_logs::export_entries_for_index)
                .service(crate::debug_logs::export_chains_for_index);
        }

        app.service(fs::Files::new("/", "./static").index_file("index.html"))
    })
    .bind(("0.0.0.0", 8080))?;

    // If IPv6 is not available do not bind it (for example inside Docker).
    if ipv6 {
        server = server.bind("[::1]:8080")?;
    }

    server.run().await
}
