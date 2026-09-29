-- An imported recording (the old client's queue, a file from another device,
-- the legacy migration) is a transport of past work, never "someone scouting
-- right now": it must not raise the live-scouting hint.
ALTER TABLE recordings ADD COLUMN imported INTEGER NOT NULL DEFAULT 0;
UPDATE recordings SET imported = 1 WHERE device_id IN ('legacy', 'import') OR device_label LIKE 'Import%';
