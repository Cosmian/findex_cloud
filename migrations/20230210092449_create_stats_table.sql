CREATE TABLE stats (
    id INTEGER PRIMARY KEY NOT NULL,
    index_id INTEGER NOT NULL,
    chains_size INTEGER NOT NULL DEFAULT(0),
    entries_size INTEGER NOT NULL DEFAULT(0),
    created_at DATETIME NOT NULL DEFAULT(current_timestamp),
    FOREIGN KEY(index_id) REFERENCES indexes(id)
)