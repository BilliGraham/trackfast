ALTER TABLE sessions ADD COLUMN duration_seconds INTEGER CHECK(duration_seconds > 0);
ALTER TABLE sessions ADD COLUMN entry_date TEXT;
ALTER TABLE sessions ADD COLUMN time_zone TEXT;
ALTER TABLE sessions ADD COLUMN round_to_five INTEGER NOT NULL DEFAULT 0 CHECK(round_to_five IN (0,1));
CREATE TABLE local_settings (
    id INTEGER PRIMARY KEY CHECK(id=1),
    round_to_five INTEGER NOT NULL CHECK(round_to_five IN (0,1)),
    weekly_target_hours REAL NOT NULL CHECK(weekly_target_hours > 0),
    glass INTEGER NOT NULL CHECK(glass IN (0,1))
);
INSERT INTO local_settings VALUES (1,1,35,0);
