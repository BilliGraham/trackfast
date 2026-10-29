CREATE TABLE native_download_cursor (id INTEGER PRIMARY KEY CHECK(id=1), cursor TEXT);
INSERT INTO native_download_cursor VALUES(1,NULL);
CREATE TABLE native_download (id INTEGER PRIMARY KEY CHECK(id=1), body TEXT NOT NULL);
CREATE TABLE native_remote_acks (token TEXT PRIMARY KEY);
