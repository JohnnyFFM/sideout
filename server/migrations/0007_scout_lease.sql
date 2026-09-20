-- Scouting handover: one active writer per match. The holder is a session
-- (a device), identified by a lease that changes with every ownership
-- period; the revision advances on every transition (claim, takeover,
-- release) and is what clients compare, separately from the match-edit
-- `version`. `scout_since` is the acquisition time shown in the banner,
-- `scout_seen` the last accepted scouting write (inactivity expiry).
ALTER TABLE matches ADD COLUMN scout_session_id INTEGER REFERENCES sessions(id) ON DELETE SET NULL;
ALTER TABLE matches ADD COLUMN scout_since TEXT;
ALTER TABLE matches ADD COLUMN scout_seen TEXT;
ALTER TABLE matches ADD COLUMN scout_lease TEXT;
ALTER TABLE matches ADD COLUMN scout_req TEXT;
ALTER TABLE matches ADD COLUMN scout_rev INTEGER NOT NULL DEFAULT 0;
