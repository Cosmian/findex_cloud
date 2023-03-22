#![feature(iter_next_chunk)]

use std::time::Duration;

#[cfg(feature = "multitenant")]
use crate::auth0::{Auth, Auth0};
#[cfg(feature = "multitenant")]
use crate::core::{Backend, BackendProject};
#[cfg(feature = "multitenant")]
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
use sqlx::{sqlite::SqlitePoolOptions, Row, SqlitePool};
use std::path::Path as FsPath;
use tokio::{task, time};

#[cfg(feature = "multitenant")]
mod auth0;
mod core;
mod errors;

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

    let indexes = sqlx::query_as!(
        Index,
        r#"
            SELECT
                *,
                COALESCE((SELECT chains_size + entries_size FROM stats WHERE id = (SELECT MAX(id) FROM stats WHERE index_id = indexes.id)), 0) as "size: _"
            FROM indexes
            WHERE project_uuid = $1 AND deleted_at IS NULL
            ORDER BY created_at DESC"#,
        project_uuid,
    )
    .fetch_all(&mut db)
    .await?;

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
                COALESCE((SELECT chains_size + entries_size FROM stats WHERE id = (SELECT MAX(id) FROM stats WHERE index_id = indexes.id)), 0) as "size: _"
            FROM indexes
            WHERE public_id = $1 AND authz_id = $2 AND deleted_at IS NULL
        "#,
        *public_id,
        authz_id,
    )
    .fetch_one(&mut db)
    .await?;

    Ok(Json(index))
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
async fn fetch_entries(pool: Data<SqlitePool>, index: Index, bytes: Bytes) -> ResponseBytes {
    let mut db = pool.acquire().await?;

    let bytes = check_body_signature(bytes, &index.public_id, &index.fetch_entries_key)?;
    let body = deserialize_set::<CoreError, Uid<UID_LENGTH>>(&bytes)?;

    let commas = vec!["?"; body.len()].join(",");
    let sql = format!("SELECT * FROM entries WHERE index_id = ? AND uid IN ({commas})");
    let mut query = sqlx::query(&sql).bind(index.id);

    for uid in &body {
        query = query.bind(uid.as_ref());
    }

    let rows = query.fetch_all(&mut db).await?;

    let uids_and_values: EncryptedTable<UID_LENGTH> = rows
        .into_iter()
        .map(|row| {
            (
                Uid::<UID_LENGTH>::try_from_bytes(row.get("uid")).unwrap(),
                row.get("value"),
            )
        })
        .collect();

    Ok(HttpResponse::Ok()
        .content_type("application/octet-stream")
        .body(uids_and_values.try_to_bytes()?))
}

#[post("/indexes/{public_id}/fetch_chains")]
async fn fetch_chains(pool: Data<SqlitePool>, index: Index, bytes: Bytes) -> ResponseBytes {
    let mut db = pool.acquire().await?;

    let bytes = check_body_signature(bytes, &index.public_id, &index.fetch_chains_key)?;
    let body = deserialize_set::<CoreError, Uid<UID_LENGTH>>(&bytes)?;

    let commas = vec!["?"; body.len()].join(",");
    let sql = format!("SELECT * FROM chains WHERE index_id = ? AND uid IN ({commas})");
    let mut query = sqlx::query(&sql).bind(index.id);

    for uid in &body {
        query = query.bind(uid.as_ref());
    }

    let rows = query.fetch_all(&mut db).await?;

    let uids_and_values: EncryptedTable<UID_LENGTH> = rows
        .into_iter()
        .map(|row| {
            (
                Uid::<UID_LENGTH>::try_from_bytes(row.get("uid")).unwrap(),
                row.get("value"),
            )
        })
        .collect();

    Ok(HttpResponse::Ok()
        .content_type("application/octet-stream")
        .body(uids_and_values.try_to_bytes()?))
}

#[post("/indexes/{public_id}/upsert_entries")]
async fn upsert_entries(pool: Data<SqlitePool>, bytes: Bytes, index: Index) -> ResponseBytes {
    let mut db = pool.acquire().await?;

    let bytes = check_body_signature(bytes, &index.public_id, &index.upsert_entries_key)?;
    let body = UpsertData::<UID_LENGTH>::try_from_bytes(&bytes)?;

    let mut rejected = EncryptedTable::with_capacity(1);

    for (uid, (old_value, new_value)) in body.iter() {
        let uid_bytes = uid.as_ref();
        let results = sqlx::query!("INSERT INTO entries (index_id, uid, value) VALUES (?, ?, ?) ON CONFLICT (index_id, uid)  DO UPDATE SET value = ? WHERE value = ?", index.id, uid_bytes, new_value, new_value, old_value).execute(&mut db).await?;

        if results.rows_affected() == 0 {
            let new_value = sqlx::query!(
                "SELECT * FROM entries WHERE index_id = ? AND uid = ?",
                index.id,
                uid_bytes,
            )
            .fetch_one(&mut db)
            .await?;

            rejected.insert(
                Uid::<UID_LENGTH>::try_from_bytes(&new_value.uid).unwrap(),
                new_value.value,
            );
        }
    }

    Ok(HttpResponse::Ok()
        .content_type("application/octet-stream")
        .body(rejected.try_to_bytes()?))
}

#[post("/indexes/{public_id}/insert_chains")]
async fn insert_chains(pool: Data<SqlitePool>, index: Index, bytes: Bytes) -> Response<()> {
    let mut db = pool.acquire().await?;

    let bytes = check_body_signature(bytes, &index.public_id, &index.insert_chains_key)?;
    let body = EncryptedTable::<UID_LENGTH>::try_from_bytes(&bytes)?;

    for (uid, value) in body.iter() {
        let uid_bytes = uid.as_ref();
        sqlx::query!(
            "INSERT OR REPLACE INTO chains (index_id, uid, value) VALUES(?, ?, ?)",
            index.id,
            uid_bytes,
            value,
        )
        .execute(&mut db)
        .await?;
    }

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

    #[cfg(feature = "multitenant")]
    let auth0 = Data::new(Auth0::from_env());

    #[cfg(feature = "multitenant")]
    let backend = Data::new(Backend::from_env());

    let database_pool = Data::new(pool.clone());

    task::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(60 * 60));

        loop {
            interval.tick().await;
            let mut db = pool.acquire().await.unwrap();
            sqlx::query!(
                "INSERT INTO stats (index_id, chains_size, entries_size)
                    SELECT index_id, SUM(chain_size) as chains_size, SUM(entry_size) as entries_size
                        FROM (
                                       SELECT index_id, LENGTH(value) as chain_size, 0 as entry_size FROM chains
                            UNION ALL  SELECT index_id, LENGTH(value) as entry_size, 0 as chain_size FROM entries
                        ) as lengths
                    GROUP BY index_id",
            ).execute(&mut db).await.unwrap();
        }
    });

    HttpServer::new(move || {
        #[allow(unused_mut)]
        let mut app = App::new()
            .wrap(Cors::permissive())
            .wrap(Logger::default())
            .app_data(database_pool.clone())
            .app_data(PayloadConfig::new(50_000_000))
            .service(get_index)
            .service(get_indexes)
            .service(post_indexes)
            .service(delete_index)
            .service(fetch_entries)
            .service(fetch_chains)
            .service(upsert_entries)
            .service(insert_chains)
            .service(fs::Files::new("/", "./static").index_file("index.html"));

        #[cfg(feature = "multitenant")]
        {
            app = app.app_data(auth0.clone());
            app = app.app_data(backend.clone());
        }

        app
    })
    .bind(("0.0.0.0", 8080))?
    .bind("[::1]:8080")?
    .run()
    .await
}
