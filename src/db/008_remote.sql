CREATE TABLE sync_remote_versions (
    entry_key TEXT PRIMARY KEY REFERENCES session_sync_identity(entry_key),
    version INTEGER NOT NULL CHECK(version > 0),
    recorded_settings TEXT NOT NULL,
    round_to_five INTEGER CHECK(round_to_five IN (0,1))
);
CREATE TABLE sync_remote_receipts (
    token TEXT PRIMARY KEY,
    fingerprint TEXT NOT NULL,
    outcome TEXT NOT NULL
);
