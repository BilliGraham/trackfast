CREATE TABLE sync_schedule (id INTEGER PRIMARY KEY CHECK(id=1), state TEXT NOT NULL);
INSERT INTO sync_schedule VALUES(1,'{"failures":0,"next_attempt_ms":0,"not_before_ms":0,"pause":null}');
