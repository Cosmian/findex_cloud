CREATE TABLE indexes (
    id INTEGER PRIMARY KEY NOT NULL,
    public_id VARCHAR UNIQUE NOT NULL,
    fetch_entries_key BLOB NOT NULL,
    fetch_chains_key BLOB NOT NULL,
    upsert_entries_key BLOB NOT NULL,
    insert_chains_key BLOB NOT NULL
)