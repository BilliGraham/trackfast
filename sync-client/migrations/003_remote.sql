CREATE TABLE remote_checkpoint (id INTEGER PRIMARY KEY CHECK(id=1), cursor TEXT);
INSERT INTO remote_checkpoint VALUES(1,NULL);
CREATE TABLE remote_conflicts (entry_id TEXT PRIMARY KEY REFERENCES entries(entry_id), body TEXT NOT NULL);
