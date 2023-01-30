CREATE TABLE IF NOT EXISTS entries (index_id INTEGER NOT NULL, uid BLOB NOT NULL, value BLOB NOT NULL);
CREATE UNIQUE INDEX idx_entries_uid ON entries (index_id, uid);