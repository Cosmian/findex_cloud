#![feature(iter_next_chunk)]
#![feature(iter_array_chunks)]

#[cfg(feature = "log_requests")]
use crate::debug_logs::DataTimeDiffInMillisecondsMutex;

use std::env;
use std::sync::Arc;

use crate::core::{IndexesDatabase, MetadataDatabase, NewIndex, Table};
use crate::errors::Error;
use actix_web::web::PayloadConfig;

use crate::{
    core::{check_body_signature, Index, MetadataCache},
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
use std::path::Path as FsPath;

mod core;
#[cfg(feature = "log_requests")]
mod debug_logs;
mod errors;
#[cfg(feature = "sqlite")]
mod sqlite;

#[cfg(feature = "heed")]
mod heed;

#[cfg(feature = "rocksdb")]
mod rocksdb;

#[cfg(feature = "dynamodb")]
mod dynamodb;

#[get("/indexes")]
async fn get_indexes(
    metadata_db: Data<dyn MetadataDatabase>,
    indexes_db: Data<dyn IndexesDatabase>,
) -> Response<Vec<Index>> {
    let mut indexes = metadata_db.get_indexes().await?;
    indexes_db.set_sizes(&mut indexes).await?;

    Ok(Json(indexes))
}

#[derive(Deserialize)]
struct PostNewIndex {
    name: String,
}

#[post("/indexes")]
async fn post_indexes(
    body: Json<PostNewIndex>,
    metadata_db: Data<dyn MetadataDatabase>,
) -> Response<Index> {
    let mut rng = CsRng::from_entropy();

    let mut fetch_entries_key = vec![0; 16];
    rng.fill_bytes(&mut fetch_entries_key);
    let mut fetch_chains_key = vec![0; 16];
    rng.fill_bytes(&mut fetch_chains_key);
    let mut upsert_entries_key = vec![0; 16];
    rng.fill_bytes(&mut upsert_entries_key);
    let mut insert_chains_key = vec![0; 16];
    rng.fill_bytes(&mut insert_chains_key);

    let id: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(5)
        .map(char::from)
        .collect();

    let index = metadata_db
        .create_index(NewIndex {
            id,
            name: body.name.clone(),
            fetch_entries_key,
            fetch_chains_key,
            upsert_entries_key,
            insert_chains_key,
        })
        .await?;

    Ok(Json(index))
}

#[get("/indexes/{id}")]
async fn get_index(
    id: Path<String>,
    metadata_cache: Data<MetadataCache>,
    metadata_db: Data<dyn MetadataDatabase>,
    indexes_db: Data<dyn IndexesDatabase>,
) -> Response<Index> {
    let index = metadata_db
        .get_index_with_cache(&metadata_cache, &id)
        .await?;

    if let Some(mut index) = index {
        indexes_db.set_size(&mut index).await?;
        Ok(Json(index))
    } else {
        Err(Error::BadRequest(format!("Unknown index for ID {id}")))
    }
}

#[delete("/indexes/{id}")]
async fn delete_index(
    id: Path<String>,
    metadata_cache: Data<MetadataCache>,
    metadata_db: Data<dyn MetadataDatabase>,
) -> Response<()> {
    metadata_db.delete_index(&id).await?;
    if let Ok(mut cache) = metadata_cache.write() {
        cache.remove(id.as_str());
    }

    Ok(Json(()))
}

#[post("/indexes/{id}/fetch_entries")]
async fn fetch_entries(
    index: Index,
    bytes: Bytes,
    indexes: Data<dyn IndexesDatabase>,
    #[cfg(feature = "log_requests")] time_diff_mutex: DataTimeDiffInMillisecondsMutex,
) -> ResponseBytes {
    let bytes = check_body_signature(bytes, &index.id, &index.fetch_entries_key)?;
    let uids = deserialize_set::<CoreError, Uid<UID_LENGTH>>(&bytes)?;

    #[cfg(feature = "log_requests")]
    let cloned_uids = uids.clone();

    let uids_and_values = indexes.fetch(&index, Table::Entries, uids).await?;

    #[cfg(feature = "log_requests")]
    crate::debug_logs::save_log(
        "fetch_entries",
        time_diff_mutex,
        cloned_uids,
        &uids_and_values,
    )?;

    // `.to_vec()` go out of the Zeroize but I don't think we can return the
    // bytes with the `HttpResponse.body()` without it.
    let bytes = uids_and_values.serialize()?.to_vec();

    Ok(HttpResponse::Ok()
        .content_type("application/octet-stream")
        .body(bytes))
}

#[post("/indexes/{id}/fetch_chains")]
async fn fetch_chains(
    index: Index,
    bytes: Bytes,
    indexes: Data<dyn IndexesDatabase>,
    #[cfg(feature = "log_requests")] time_diff_mutex: DataTimeDiffInMillisecondsMutex,
) -> ResponseBytes {
    let bytes = check_body_signature(bytes, &index.id, &index.fetch_chains_key)?;
    let uids = deserialize_set::<CoreError, Uid<UID_LENGTH>>(&bytes)?;

    #[cfg(feature = "log_requests")]
    let cloned_uids = uids.clone();

    let uids_and_values = indexes.fetch(&index, Table::Chains, uids).await?;

    #[cfg(feature = "log_requests")]
    crate::debug_logs::save_log(
        "fetch_chains",
        time_diff_mutex,
        cloned_uids,
        &uids_and_values,
    )?;

    // `.to_vec()` go out of the Zeroize but I don't think we can return the
    // bytes with the `HttpResponse.body()` without it.
    let bytes = uids_and_values.serialize()?.to_vec();

    Ok(HttpResponse::Ok()
        .content_type("application/octet-stream")
        .body(bytes))
}

#[post("/indexes/{id}/upsert_entries")]
async fn upsert_entries(
    bytes: Bytes,
    index: Index,
    indexes: Data<dyn IndexesDatabase>,
) -> ResponseBytes {
    let bytes = check_body_signature(bytes, &index.id, &index.upsert_entries_key)?;
    let data = UpsertData::<UID_LENGTH>::deserialize(&bytes)?;

    let rejected = indexes.upsert_entries(&index, data).await?;

    // `.to_vec()` go out of the Zeroize but I don't think we can return the
    // bytes with the `HttpResponse.body()` without it.
    let bytes = rejected.serialize()?.to_vec();

    Ok(HttpResponse::Ok()
        .content_type("application/octet-stream")
        .body(bytes))
}

#[post("/indexes/{id}/insert_chains")]
async fn insert_chains(
    index: Index,
    bytes: Bytes,
    indexes: Data<dyn IndexesDatabase>,
) -> Response<()> {
    let bytes = check_body_signature(bytes, &index.id, &index.insert_chains_key)?;
    let data = EncryptedTable::<UID_LENGTH>::deserialize(&bytes)?;

    indexes.insert_chains(&index, data).await?;

    Ok(Json(()))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    if FsPath::new(".env").exists() {
        dotenv::dotenv().expect("Cannot load env");
    }

    env_logger::Builder::from_env(Env::default().default_filter_or("debug")).init();

    match start_server(true).await {
        Ok(_) => Ok(()),
        Err(_) => start_server(false).await,
    }
}

async fn start_server(ipv6: bool) -> std::io::Result<()> {
    let metadata_cache: Data<MetadataCache> = Data::new(Default::default());

    let indexes_database: Data<dyn IndexesDatabase> = match env::var("INDEXES_DATABASE_TYPE").as_deref().unwrap_or("rocksdb") {
            #[cfg(feature = "heed")]
            "heed" => Data::from(Arc::new(crate::heed::Database::create()) as Arc<dyn IndexesDatabase>),
            #[cfg(not(feature = "heed"))]
            "heed" => panic!("Cannot load `INDEXES_DATABASE_TYPE=heed` because `findex_cloud` wasn't compiled with \"heed\" feature."),

            #[cfg(feature = "rocksdb")]
            "rocksdb" => Data::from(Arc::new(crate::rocksdb::Database::create()) as Arc<dyn IndexesDatabase>),
            #[cfg(not(feature = "rocksdb"))]
            "rocksdb" => panic!("Cannot load `INDEXES_DATABASE_TYPE=rocksdb` because `findex_cloud` wasn't compiled with \"rocksdb\" feature."),

            #[cfg(feature = "dynamodb")]
            "dynamodb" => Data::from(Arc::new(crate::dynamodb::Database::create().await) as Arc<dyn IndexesDatabase>),
            #[cfg(not(feature = "dynamodb"))]
            "dynamodb" => panic!("Cannot load `INDEXES_DATABASE_TYPE=dynamodb` because `findex_cloud` wasn't compiled with \"dynamodb\" feature."),

            indexes_database_type => panic!("Unknown `INDEXES_DATABASE_TYPE` env variable `{indexes_database_type}` (please use `rocksdb`, `dynamodb` or `heed`)"),
        };

    let metadata_database: Data<dyn MetadataDatabase> = match env::var("METADATA_DATABASE_TYPE").as_deref().unwrap_or("sqlite") {
            #[cfg(feature = "sqlite")]
            "sqlite" => Data::from(Arc::new(crate::sqlite::Database::create().await) as Arc<dyn MetadataDatabase>),
            #[cfg(not(feature = "sqlite"))]
            "sqlite" => panic!("Cannot load `METADATA_DATABASE_TYPE=sqlite` because `findex_cloud` wasn't compiled with \"sqlite\" feature."),

            #[cfg(feature = "dynamodb")]
            "dynamodb" => Data::from(Arc::new(crate::dynamodb::Database::create().await) as Arc<dyn MetadataDatabase>),
            #[cfg(not(feature = "dynamodb"))]
            "dynamodb" => panic!("Cannot load `METADATA_DATABASE_TYPE=dynamodb` because `findex_cloud` wasn't compiled with \"dynamodb\" feature."),

            metadata_database_type => panic!("Unknown `METADATA_DATABASE_TYPE` env variable `{metadata_database_type}` (please use `sqlite`)"),
        };

    #[cfg(feature = "log_requests")]
    let time_mock: DataTimeDiffInMillisecondsMutex = Data::new(Default::default());

    let mut server = HttpServer::new(move || {
        #[allow(unused_mut)]
        let mut app = App::new()
            .wrap(Cors::permissive())
            .wrap(Logger::default())
            .app_data(metadata_cache.clone())
            .app_data(indexes_database.clone())
            .app_data(metadata_database.clone())
            .app_data(PayloadConfig::new(50_000_000))
            .service(get_index)
            .service(get_indexes)
            .service(post_indexes)
            .service(delete_index)
            .service(fetch_entries)
            .service(fetch_chains)
            .service(upsert_entries)
            .service(insert_chains);

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
