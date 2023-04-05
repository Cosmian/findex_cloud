/// This module is only used to log all requests and export the database.
/// Its feature SHOULD never be activate on production.
/// We currently use this feature to generate data to run attack scripts on it
/// and verify the security of Findex.
///
/// `set_time_diff` and `TimeDiffInMilliseconds` allow to change the current time of
/// the logged request to let the client determine the starting time for each request
/// while keeping the correct difference between the fetch_entries and fetch_chains calls.
///
/// Requests logs are JSON encoded lines to easy append a new line to the file. `get_requests_log`
/// will convert these JSON lines to a correct JSON array (adding the `[]` around the file and
/// the `,` between each lines)
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::{sync::RwLock, time::SystemTime};

use actix_web::{
    get, post,
    web::{Data, Json, Path},
};
use base64::{engine::general_purpose, Engine as _};
use cosmian_findex::{parameters::UID_LENGTH, Uid};
use rocksdb::TransactionDB;

use crate::{
    core::{Index, Table},
    errors::{Error, Response},
};

const LOGS_PATH: &str = "data/requests.log";

pub(crate) type DataTimeDiffInMillisecondsMutex = Data<RwLock<TimeDiffInMilliseconds>>;

#[derive(Default)]
pub(crate) struct TimeDiffInMilliseconds(pub(crate) i128);

#[post("/set_time_diff/{fake_time}")]
pub(crate) async fn set_time_diff(
    fake_time: Path<String>,
    time_diff_mutex: Data<RwLock<TimeDiffInMilliseconds>>,
) -> Response<()> {
    let fake_time_in_milliseconds: u128 = fake_time
        .parse()
        .map_err(|_| Error::BadRequest(format!("Cannot parse fake_time {fake_time}")))?;

    let current_time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|_| Error::BadRequest("SystemTime is before UNIX_EPOCH".to_owned()))?;

    {
        let mut time_diff = time_diff_mutex.write().unwrap();
        time_diff.0 = current_time.as_millis() as i128 - fake_time_in_milliseconds as i128;
    }

    Ok(Json(()))
}

#[get("/requests_log")]
pub(crate) async fn get_requests_log() -> String {
    let contents = std::fs::read_to_string(LOGS_PATH).unwrap_or("".to_owned());

    let contents_with_commas = contents.lines().collect::<Vec<_>>().join(",\n");

    format!("[{contents_with_commas}]")
}

#[get("/export_entries_for_index/{public_id}")]
pub(crate) async fn export_entries_for_index(index: Index, indexes: Data<TransactionDB>) -> String {
    export_all_indexes_for_prefix(&index, Table::Entries, indexes)
}

#[get("/export_chains_for_index/{public_id}")]
pub(crate) async fn export_chains_for_index(index: Index, indexes: Data<TransactionDB>) -> String {
    export_all_indexes_for_prefix(&index, Table::Chains, indexes)
}

#[post("/reset_requests_log")]
async fn post_reset_requests_log() -> String {
    let _ = std::fs::remove_file(LOGS_PATH); // Don't want to crash if the file doesn't exists
    "OK".to_owned()
}

fn rocksdb_keys_prefix(index: &Index, table: Table) -> Vec<u8> {
    [&index.id.to_be_bytes(), &[table as u8][..]].concat()
}

pub(crate) fn save_log(
    log_type: &str,
    time_diff_mutex: Data<std::sync::RwLock<TimeDiffInMilliseconds>>,
    uids: std::collections::HashSet<Uid<UID_LENGTH>>,
    uids_and_values: &cosmian_findex::EncryptedTable<UID_LENGTH>,
) {
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .append(true)
        .open(LOGS_PATH)
        .unwrap();

    let current_time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|_| Error::BadRequest("SystemTime is before UNIX_EPOCH".to_owned()))
        .unwrap();

    let data: HashMap<String, Option<String>> = uids
        .iter()
        .map(|uid| {
            (
                general_purpose::STANDARD_NO_PAD.encode(uid),
                uids_and_values
                    .get(uid)
                    .map(|uid| general_purpose::STANDARD_NO_PAD.encode(uid)),
            )
        })
        .collect();

    // Lock for writing to prevent writing two lines at once inside file
    // This is sub-optimal since it put a sync point between requests that
    // could change timing patterns.
    let time_diff = time_diff_mutex.write().unwrap();
    let timestamp = current_time.as_millis() as i128 + time_diff.0;

    let json = serde_json::json!({
        "date": timestamp,
        "type": log_type,
        "data": data,
    });
    writeln!(file, "{}", serde_json::to_string(&json).unwrap()).unwrap();
}

fn export_all_indexes_for_prefix(
    index: &Index,
    table: Table,
    indexes: Data<rocksdb::TransactionDB>,
) -> String {
    let prefix = rocksdb_keys_prefix(index, table);

    let iter = indexes.iterator(rocksdb::IteratorMode::From(
        &prefix,
        rocksdb::Direction::Forward,
    ));

    let contents_with_commas = iter
        .filter_map(|result| result.ok())
        .take_while(|(key, _)| key.starts_with(&prefix))
        .map(|(key, value)| {
            format!(
                "\"{}\":\"{}\"",
                general_purpose::STANDARD_NO_PAD.encode(key),
                general_purpose::STANDARD_NO_PAD.encode(value)
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");

    format!("[{contents_with_commas}]")
}