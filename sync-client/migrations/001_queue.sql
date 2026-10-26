CREATE TABLE scope (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    value TEXT NOT NULL
);
CREATE TABLE entries (
    entry_id TEXT PRIMARY KEY NOT NULL,
    body TEXT NOT NULL
);
CREATE TABLE pending (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    operation_id TEXT NOT NULL UNIQUE,
    entry_id TEXT NOT NULL UNIQUE REFERENCES entries(entry_id)
);
