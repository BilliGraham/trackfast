CREATE TABLE native_resolutions (
    operation_id TEXT PRIMARY KEY REFERENCES native_transfers(operation_id),
    retry_operation_id TEXT UNIQUE,
    body TEXT NOT NULL
);
