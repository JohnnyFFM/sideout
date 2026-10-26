-- A deleted team keeps its row as a tombstone: users.team_id is NOT NULL
-- with ON DELETE CASCADE, so a real delete would take every account whose
-- default team it is with it. Its contents (matches, recordings, players,
-- memberships) are deleted for real; the join code stops working.
ALTER TABLE teams ADD COLUMN deleted_at TEXT;
