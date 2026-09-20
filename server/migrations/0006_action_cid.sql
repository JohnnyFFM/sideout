-- client-generated id per action: lets a phone reconcile its offline queue
-- against the log after a lost response (add already saved? undo already done?)
ALTER TABLE actions ADD COLUMN cid TEXT;
CREATE UNIQUE INDEX idx_actions_cid ON actions(match_id, cid) WHERE cid IS NOT NULL;
