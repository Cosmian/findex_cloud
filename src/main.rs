use actix_cors::Cors;
use actix_web::{
    patch, post,
    web::{Data, Json},
    App, HttpServer, Responder,
};
use env_logger::Env;
use p384::ecdsa::VerifyingKey;
use rand::{distributions::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};

#[derive(Deserialize)]
struct PostIndexes {
    public_key: String,
}

#[derive(Serialize)]
struct Index {
    id: i64,
    public_id: Option<String>,
    public_key: Option<Vec<u8>>,
}

#[post("/indexes")]
async fn post_indexes(body: Json<PostIndexes>, pool: Data<SqlitePool>) -> impl Responder {
    let mut db = pool.acquire().await.unwrap();

    let public_key = hex::decode(&body.public_key).unwrap();

    let verifying_key = VerifyingKey::from_sec1_bytes(&public_key).expect("Cannot parse key");

    let public_id: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(5)
        .map(char::from)
        .collect();

    let index = sqlx::query_as!(
        Index,
        r#"INSERT INTO indexes (public_id, public_key) VALUES ($1, $2) RETURNING *"#,
        public_id,
        public_key,
    )
    .fetch_one(&mut db)
    .await
    .unwrap();

    std::fs::File::create(&format!("databases/{}.sqlite", index.id))
        .expect("Cannot create database file");

    let index_database = SqlitePoolOptions::new()
        .connect(&format!("sqlite://databases/{}.sqlite", index.id))
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

#[post("/entries")]
async fn get_entries(body: Json<Vec<String>>, pool: Data<SqlitePool>) -> impl Responder {
    let response: Vec<UidAndValue> = vec![];

    Json(response)
}

#[derive(Serialize, Deserialize)]
struct UidAndOldAndNewValues {
    uid: String,
    old_value: Option<String>,
    new_value: String,
}

#[patch("/entries")]
async fn upsert_entries(
    body: Json<Vec<UidAndOldAndNewValues>>,
    pool: Data<SqlitePool>,
) -> impl Responder {
    let response: Vec<UidAndValue> = vec![];

    for UidAndOldAndNewValues { uid, new_value, .. } in &*body {}

    Json(response)
}

#[patch("/chains")]
async fn insert_chains(body: Json<Vec<UidAndValue>>, pool: Data<SqlitePool>) -> impl Responder {
    let response: Vec<UidAndValue> = vec![];

    Json(response)
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
            .service(get_entries)
            .service(upsert_entries)
            .service(insert_chains)
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
