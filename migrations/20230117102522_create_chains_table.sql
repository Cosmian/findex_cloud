CREATE TABLE IF NOT EXISTS chains (index_id INTEGER NOT NULL, uid BLOB NOT NULL, value BLOB NOT NULL);
CREATE UNIQUE INDEX idx_chains_uid ON chains (index_id, uid);