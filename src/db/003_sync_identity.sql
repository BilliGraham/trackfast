CREATE TABLE session_sync_identity (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    entry_key TEXT NOT NULL UNIQUE DEFAULT (lower(hex(randomblob(16)))),
    session_id INTEGER UNIQUE REFERENCES sessions(id),
    revision INTEGER NOT NULL DEFAULT 1 CHECK(typeof(revision) = 'integer' AND revision > 0)
);

INSERT INTO session_sync_identity(session_id) SELECT id FROM sessions ORDER BY id;

CREATE TRIGGER sessions_sync_insert AFTER INSERT ON sessions
BEGIN
    INSERT INTO session_sync_identity(session_id) VALUES (NEW.id);
END;

CREATE TRIGGER sessions_sync_update AFTER UPDATE ON sessions
WHEN OLD.description IS NOT NEW.description
    OR OLD.project_id IS NOT NEW.project_id
    OR OLD.start_utc IS NOT NEW.start_utc
    OR OLD.stop_utc IS NOT NEW.stop_utc
    OR OLD.duration_seconds IS NOT NEW.duration_seconds
    OR OLD.entry_date IS NOT NEW.entry_date
    OR OLD.time_zone IS NOT NEW.time_zone
    OR OLD.round_to_five IS NOT NEW.round_to_five
    OR OLD.toggl_id IS NOT NEW.toggl_id
BEGIN
    UPDATE session_sync_identity SET revision = revision + 1 WHERE session_id = NEW.id;
END;

CREATE TRIGGER sessions_sync_delete BEFORE DELETE ON sessions
BEGIN
    UPDATE session_sync_identity SET session_id = NULL, revision = revision + 1
    WHERE session_id = OLD.id;
END;
