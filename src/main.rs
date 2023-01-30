use actix_cors::Cors;
use actix_web::{
    post,
    web::{Bytes, Data, Json},
    App, HttpRequest, HttpServer,
};
use cosmian_crypto_core::CsRng;
use env_logger::Env;
use rand::{distributions::Alphanumeric, Rng, RngCore, SeedableRng};
use sqlx::{sqlite::SqlitePoolOptions, Row, SqlitePool};

use crate::{
    core::{parse_body_with_signature, Id, Index, UidAndOldAndNewValues, UidAndValue},
    errors::Response,
};

mod core;
mod errors;

#[post("/indexes")]
async fn post_indexes(pool: Data<SqlitePool>) -> Response<Index> {
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
            public_id,
            fetch_entries_key,
            fetch_chains_key,
            upsert_entries_key,
            insert_chains_key
        ) VALUES ($1, $2, $3, $4, $5) RETURNING id"#,
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
) -> Response<Vec<UidAndValue>> {
    let mut db = pool.acquire().await?;

    let body: Vec<String> = parse_body_with_signature(&request, bytes, &index.fetch_entries_key)?;

    let commas = vec!["?"; body.len()].join(",");
    let sql = format!("SELECT * FROM entries WHERE index_id = ? AND uid IN ({commas})");
    let mut query = sqlx::query(&sql).bind(index.id);

    for uid in &*body {
        query = query.bind(hex::decode(uid)?);
    }

    let rows = query.fetch_all(&mut db).await?;

    let uids_and_values: Vec<_> = rows
        .into_iter()
        .map(|row| UidAndValue {
            uid: hex::encode::<Vec<u8>>(row.get("uid")),
            value: hex::encode::<Vec<u8>>(row.get("value")),
        })
        .collect();

    Ok(Json(uids_and_values))
}

#[post("/indexes/{public_id}/fetch_chains")]
async fn fetch_chains(
    pool: Data<SqlitePool>,
    index: Index,
    bytes: Bytes,
    request: HttpRequest,
) -> Response<Vec<UidAndValue>> {
    let mut db = pool.acquire().await?;

    let body: Vec<String> = parse_body_with_signature(&request, bytes, &index.fetch_chains_key)?;

    let commas = vec!["?"; body.len()].join(",");
    let sql = format!("SELECT * FROM chains WHERE index_id = ? AND uid IN ({commas})");
    let mut query = sqlx::query(&sql).bind(index.id);

    for uid in &*body {
        query = query.bind(hex::decode(uid)?);
    }

    let rows = query.fetch_all(&mut db).await?;

    let uids_and_values: Vec<_> = rows
        .into_iter()
        .map(|row| UidAndValue {
            uid: hex::encode::<Vec<u8>>(row.get("uid")),
            value: hex::encode::<Vec<u8>>(row.get("value")),
        })
        .collect();

    Ok(Json(uids_and_values))
}

#[post("/indexes/{public_id}/upsert_entries")]
async fn upsert_entries(
    pool: Data<SqlitePool>,
    bytes: Bytes,
    request: HttpRequest,
    index: Index,
) -> Response<Vec<UidAndValue>> {
    let mut db = pool.acquire().await?;

    let body: Vec<UidAndOldAndNewValues> =
        parse_body_with_signature(&request, bytes, &index.upsert_entries_key)?;

    let sql = "INSERT INTO entries (index_id, uid, value) VALUES (?, ?, ?) ON CONFLICT (index_id, uid)  DO UPDATE SET value = ? WHERE value = ?";
    let mut rejected = vec![];

    for info in &*body {
        let mut query = sqlx::query(sql);
        query = query.bind(index.id);
        query = query.bind(hex::decode(&info.uid)?);
        query = query.bind(hex::decode(&info.new_value)?);
        query = query.bind(hex::decode(&info.new_value)?);
        query = query.bind(
            info.old_value
                .clone()
                .map(|old_value| hex::decode(old_value))
                .map_or(Ok(None), |v| v.map(Some))? // option<result> to result<option>
                .unwrap_or_default(),
        );

        let results = query.execute(&mut db).await?;

        if results.rows_affected() == 0 {
            let uid_bytes = hex::decode(&info.uid)?;

            let new_value = sqlx::query!(
                "SELECT * FROM entries WHERE index_id = ? AND uid = ?",
                index.id,
                uid_bytes,
            )
            .fetch_one(&mut db)
            .await?;

            rejected.push(UidAndValue {
                uid: hex::encode::<Vec<u8>>(new_value.uid),
                value: hex::encode::<Vec<u8>>(new_value.value),
            });
        }
    }

    Ok(Json(rejected))
}

#[post("/indexes/{public_id}/insert_chains")]
async fn insert_chains(
    pool: Data<SqlitePool>,
    index: Index,
    bytes: Bytes,
    request: HttpRequest,
) -> Response<()> {
    let mut db = pool.acquire().await?;

    let body: Vec<UidAndValue> =
        parse_body_with_signature(&request, bytes, &index.insert_chains_key)?;

    let sql = "INSERT OR REPLACE INTO chains (index_id, uid, value) VALUES(?, ?, ?)";

    for info in &*body {
        let mut query = sqlx::query(sql);
        query = query.bind(index.id);
        query = query.bind(hex::decode(&info.uid)?);
        query = query.bind(hex::decode(&info.value)?);

        query.execute(&mut db).await?;
    }

    Ok(Json(()))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("debug")).init();
    let pool = SqlitePoolOptions::new()
        .connect("sqlite://database.sqlite")
        .await
        .expect("Cannot connect to database.sqlite");

    sqlx::migrate!()
        .run(&pool)
        .await
        .expect("Cannot run the database migrations");

    let database_pool = Data::new(pool.clone());
    HttpServer::new(move || {
        App::new()
            .wrap(Cors::permissive())
            .app_data(database_pool.clone())
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
