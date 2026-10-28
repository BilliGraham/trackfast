CREATE TABLE native_transfers (
    operation_id TEXT PRIMARY KEY,
    fingerprint TEXT NOT NULL,
    confirmed INTEGER NOT NULL DEFAULT 0 CHECK(confirmed IN (0,1))
);
