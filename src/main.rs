use std::time::Duration;

use crate::{
    auth0::{Auth, Auth0},
    core::{check_body_signature, Backend, BackendProject, Id, Index},
    errors::{Error, Response, ResponseBytes},
};
use actix_cors::Cors;
use actix_web::{
    get, post,
    web::{Bytes, Data, Json, Path},
    App, HttpRequest, HttpResponse, HttpServer,
};
use cosmian_crypto_core::{bytes_ser_de::Serializable, CsRng};
use cosmian_findex::{
    core::{EncryptedTable, Uid, UpsertData},
    interfaces::{generic_parameters::UID_LENGTH, ser_de::deserialize_set},
};
use env_logger::Env;
use rand::{distributions::Alphanumeric, Rng, RngCore, SeedableRng};
use sqlx::{sqlite::SqlitePoolOptions, Row, SqlitePool};
use tokio::{task, time};

mod auth0;
mod core;
mod errors;

#[get("/projects/{project_uuid}/indexes")]
async fn get_indexes(
    pool: Data<SqlitePool>,
    backend: Data<Backend>,
    auth: Auth,
    project_uuid: Path<String>,
) -> Response<Vec<Index>> {
    let projects = BackendProject::get_projects(&backend, &auth).await?;

    if !projects.contains(&BackendProject {
        uuid: project_uuid.clone(),
    }) {
        return Err(Error::UnknownProject(project_uuid.clone()));
    }

    let mut db = pool.acquire().await?;

    let indexes = sqlx::query_as!(
        Index,
        r#"SELECT * FROM indexes WHERE project_uuid = $1"#,
        *project_uuid,
    )
    .fetch_all(&mut db)
    .await?;

    Ok(Json(indexes))
}

#[post("/projects/{project_uuid}/indexes")]
async fn post_indexes(
    pool: Data<SqlitePool>,
    backend: Data<Backend>,
    auth: Auth,
    project_uuid: Path<String>,
) -> Response<Index> {
    let projects = BackendProject::get_projects(&backend, &auth).await?;

    if !projects.contains(&BackendProject {
        uuid: project_uuid.clone(),
    }) {
        return Err(Error::UnknownProject(project_uuid.clone()));
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

    let Id { id } = sqlx::query_as!(
        Id,
        r#"INSERT INTO indexes (
            authz_id,
            project_uuid,
            public_id,
            fetch_entries_key,
            fetch_chains_key,
            upsert_entries_key,
            insert_chains_key
        ) VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id"#,
        auth.authz_id,
        *project_uuid,
        public_id,
        fetch_entries_key,
        fetch_chains_key,
        upsert_entries_key,
        insert_chains_key,
    )
    .fetch_one(&mut db)
    .await?;

    let index = sqlx::query_as!(Index, r#"SELECT * FROM indexes WHERE id = $1"#, id)
        .fetch_one(&mut db)
        .await?;

    Ok(Json(index))
}

#[post("/indexes/{public_id}/fetch_entries")]
async fn fetch_entries(
    pool: Data<SqlitePool>,
    index: Index,
    bytes: Bytes,
    request: HttpRequest,
) -> ResponseBytes {
    let mut db = pool.acquire().await?;

    check_body_signature(&request, &bytes, &index.fetch_entries_key)?;
    let body = deserialize_set::<Uid<UID_LENGTH>>(&bytes)?;

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
async fn fetch_chains(
    pool: Data<SqlitePool>,
    index: Index,
    bytes: Bytes,
    request: HttpRequest,
) -> ResponseBytes {
    let mut db = pool.acquire().await?;

    check_body_signature(&request, &bytes, &index.fetch_chains_key)?;
    let body = deserialize_set::<Uid<UID_LENGTH>>(&bytes)?;

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
async fn upsert_entries(
    pool: Data<SqlitePool>,
    bytes: Bytes,
    request: HttpRequest,
    index: Index,
) -> ResponseBytes {
    let mut db = pool.acquire().await?;

    check_body_signature(&request, &bytes, &index.upsert_entries_key)?;
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
async fn insert_chains(
    pool: Data<SqlitePool>,
    index: Index,
    bytes: Bytes,
    request: HttpRequest,
) -> Response<()> {
    let mut db = pool.acquire().await?;

    check_body_signature(&request, &bytes, &index.insert_chains_key)?;
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
    dotenv::dotenv().expect("Cannot load env");

    env_logger::Builder::from_env(Env::default().default_filter_or("debug")).init();
    let pool = SqlitePoolOptions::new()
        .connect("sqlite://database.sqlite")
        .await
        .expect("Cannot connect to database.sqlite");

    sqlx::migrate!()
        .run(&pool)
        .await
        .expect("Cannot run the database migrations");

    let auth0 = Data::new(Auth0::from_env());
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
                            UNION   SELECT index_id, LENGTH(value) as entry_size, 0 as chain_size FROM entries
                        ) as lengths
                    GROUP BY index_id",
            ).execute(&mut db).await.unwrap();
        }
    });

    HttpServer::new(move || {
        App::new()
            .wrap(Cors::permissive())
            .app_data(database_pool.clone())
            .app_data(backend.clone())
            .app_data(auth0.clone())
            .service(get_indexes)
            .service(post_indexes)
            .service(fetch_entries)
            .service(fetch_chains)
            .service(upsert_entries)
            .service(insert_chains)
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
