CREATE TABLE indexes (
    id INTEGER PRIMARY KEY NOT NULL,
    public_id VARCHAR UNIQUE NOT NULL,

    authz_id VARCHAR NOT NULL,
    project_uuid VARCHAR NOT NULL,

    name VARCHAR NOT NULL,

    fetch_entries_key BLOB NOT NULL,
    fetch_chains_key BLOB NOT NULL,
    upsert_entries_key BLOB NOT NULL,
    insert_chains_key BLOB NOT NULL,
    created_at DATETIME NOT NULL DEFAULT(current_timestamp) ,
    deleted_at DATETIME
)