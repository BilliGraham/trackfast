CREATE TABLE sync_delivered (
    entry_key TEXT PRIMARY KEY REFERENCES session_sync_identity(entry_key),
    revision INTEGER NOT NULL CHECK(revision > 0)
);
CREATE TABLE sync_outbox (
    id INTEGER PRIMARY KEY CHECK(id=1),
    entry_key TEXT NOT NULL REFERENCES session_sync_identity(entry_key),
    revision INTEGER NOT NULL CHECK(revision > 0),
    body TEXT NOT NULL
);
