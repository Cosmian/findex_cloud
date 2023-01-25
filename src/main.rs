use actix_cors::Cors;
use actix_web::{
    post,
    web::{Data, Json, Path},
    App, HttpServer, Responder,
};
use cosmian_crypto_core::CsRng;
use env_logger::Env;
use rand::{distributions::Alphanumeric, Rng, RngCore, SeedableRng};
use serde::{Deserialize, Serialize};
use sqlx::{sqlite::SqlitePoolOptions, Row, SqlitePool};

struct Id {
    id: i64,
}

#[derive(Serialize)]
struct Index {
    id: i64,
    public_id: String,
    fetch_entries_key: Vec<u8>,
    fetch_chains_key: Vec<u8>,
    upsert_entries_key: Vec<u8>,
    insert_chains_key: Vec<u8>,
}

#[post("/indexes")]
async fn post_indexes(pool: Data<SqlitePool>) -> impl Responder {
    let mut db = pool.acquire().await.unwrap();

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
    .await
    .unwrap();

    let index = sqlx::query_as!(Index, r#"SELECT * FROM indexes WHERE id = $1"#, id)
        .fetch_one(&mut db)
        .await
        .unwrap();

    std::fs::File::create(&format!("databases/{}.sqlite", index.public_id))
        .expect("Cannot create database file");

    let index_database = SqlitePoolOptions::new()
        .connect(&format!("sqlite://databases/{}.sqlite", index.public_id))
        .await
        .expect("Cannot connect to the index database");

    sqlx::migrate!("./migrations_index")
        .run(&index_database)
        .await
        .expect("Cannot run the database migrations on index database");

    Json(index)
}

#[derive(Serialize, Deserialize)]
struct UidAndValue {
    uid: String,
    value: String,
}

#[post("/indexes/{public_id}/fetch_entries")]
async fn fetch_entries(path: Path<String>, body: Json<Vec<String>>) -> impl Responder {
    let index_pool = SqlitePoolOptions::new()
        .connect(&format!("sqlite://databases/{}.sqlite", *path))
        .await
        .expect("Cannot connect to the index database");
    let mut index_database = index_pool.acquire().await.unwrap();

    let commas = vec!["?"; body.len()].join(",");
    let sql = format!("SELECT * FROM entries WHERE uid IN ({commas})");
    let mut query = sqlx::query(&sql);

    for uid in &*body {
        query = query.bind(hex::decode(uid).unwrap());
    }

    let rows = query.fetch_all(&mut index_database).await.unwrap();

    let uids_and_values: Vec<_> = rows
        .into_iter()
        .map(|row| UidAndValue {
            uid: hex::encode::<Vec<u8>>(row.get("uid")),
            value: hex::encode::<Vec<u8>>(row.get("value")),
        })
        .collect();

    Json(uids_and_values)
}

#[post("/indexes/{public_id}/fetch_chains")]
async fn fetch_chains(path: Path<String>, body: Json<Vec<String>>) -> impl Responder {
    let index_pool = SqlitePoolOptions::new()
        .connect(&format!("sqlite://databases/{}.sqlite", *path))
        .await
        .expect("Cannot connect to the index database");
    let mut index_database = index_pool.acquire().await.unwrap();

    let commas = vec!["?"; body.len()].join(",");
    let sql = format!("SELECT * FROM chains WHERE uid IN ({commas})");
    let mut query = sqlx::query(&sql);

    for uid in &*body {
        query = query.bind(hex::decode(uid).unwrap());
    }

    let rows = query.fetch_all(&mut index_database).await.unwrap();

    let uids_and_values: Vec<_> = rows
        .into_iter()
        .map(|row| UidAndValue {
            uid: hex::encode::<Vec<u8>>(row.get("uid")),
            value: hex::encode::<Vec<u8>>(row.get("value")),
        })
        .collect();

    Json(uids_and_values)
}

#[derive(Serialize, Deserialize)]
struct UidAndOldAndNewValues {
    uid: String,
    old_value: Option<String>,
    new_value: String,
}

#[post("/indexes/{public_id}/upsert_entries")]
async fn upsert_entries(
    path: Path<String>,
    body: Json<Vec<UidAndOldAndNewValues>>,
) -> impl Responder {
    let index_pool = SqlitePoolOptions::new()
        .connect(&format!("sqlite://databases/{}.sqlite", *path))
        .await
        .expect("Cannot connect to the index database");
    let mut index_database = index_pool.acquire().await.unwrap();

    let sql = format!("INSERT INTO entries (uid, value) VALUES (?, ?) ON CONFLICT (uid)  DO UPDATE SET value = ? WHERE value = ?");
    let mut rejected = vec![];

    for info in &*body {
        let mut query = sqlx::query(&sql);
        query = query.bind(hex::decode(&info.uid).unwrap());
        query = query.bind(hex::decode(&info.new_value).unwrap());
        query = query.bind(hex::decode(&info.new_value).unwrap());
        query = query.bind(
            info.old_value
                .clone()
                .map(|old_value| hex::decode(old_value).unwrap())
                .unwrap_or(vec![]),
        );

        let results = query.execute(&mut index_database).await.unwrap();

        if results.rows_affected() == 0 {
            let sql = format!("SELECT * FROM entries WHERE uid = ?");
            let new_value = sqlx::query(&sql)
                .bind(hex::decode(&info.uid).unwrap())
                .fetch_one(&mut index_database)
                .await
                .unwrap();

            rejected.push(UidAndValue {
                uid: hex::encode::<Vec<u8>>(new_value.get("uid")),
                value: hex::encode::<Vec<u8>>(new_value.get("value")),
            });
        }
    }

    Json(rejected)
}

#[post("/indexes/{public_id}/insert_chains")]
async fn insert_chains(path: Path<String>, body: Json<Vec<UidAndValue>>) -> impl Responder {
    let index_pool = SqlitePoolOptions::new()
        .connect(&format!("sqlite://databases/{}.sqlite", *path))
        .await
        .expect("Cannot connect to the index database");
    let mut index_database = index_pool.acquire().await.unwrap();

    let sql = format!("INSERT OR REPLACE INTO chains (uid, value) VALUES(?, ?)");

    for info in &*body {
        let mut query = sqlx::query(&sql);
        query = query.bind(hex::decode(&info.uid).unwrap());
        query = query.bind(hex::decode(&info.value).unwrap());

        query.execute(&mut index_database).await.unwrap();
    }

    ""
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
