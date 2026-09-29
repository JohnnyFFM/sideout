-- A recording can be deleted by a coach of the team or by its creator, never
-- the selected one. Deletion is a flag: the rows stay (an offline device may
-- still upload into it), the recording is hidden from every list.
ALTER TABLE recordings ADD COLUMN deleted INTEGER NOT NULL DEFAULT 0;
