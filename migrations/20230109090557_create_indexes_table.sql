CREATE TABLE indexes (
    id VARCHAR PRIMARY KEY NOT NULL,
    name VARCHAR NOT NULL,
    fetch_entries_key BLOB NOT NULL,
    fetch_chains_key BLOB NOT NULL,
    upsert_entries_key BLOB NOT NULL,
    insert_chains_key BLOB NOT NULL,
    created_at DATETIME NOT NULL DEFAULT(current_timestamp)
)