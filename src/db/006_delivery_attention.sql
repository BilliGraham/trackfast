ALTER TABLE sync_outbox ADD COLUMN needs_attention INTEGER NOT NULL DEFAULT 0 CHECK(needs_attention IN (0,1));
