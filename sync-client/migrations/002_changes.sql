ALTER TABLE entries ADD COLUMN server_version INTEGER NOT NULL DEFAULT 0;
ALTER TABLE entries ADD COLUMN deleted INTEGER NOT NULL DEFAULT 0 CHECK (deleted IN (0, 1));
ALTER TABLE pending ADD COLUMN action TEXT NOT NULL DEFAULT 'create' CHECK (action IN ('create', 'replace', 'delete'));
ALTER TABLE pending ADD COLUMN expected_version INTEGER;
ALTER TABLE pending ADD COLUMN conflict INTEGER NOT NULL DEFAULT 0 CHECK (conflict IN (0, 1));
UPDATE entries SET server_version = 1 WHERE entry_id NOT IN (SELECT entry_id FROM pending);
