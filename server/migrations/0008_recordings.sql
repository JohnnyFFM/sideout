-- Recordings replace the single shared action log and the scouting lease.
-- One recording = one scout on one device: an immutable initial state
-- (`base`, canonical JSON: first serve, lineups, roster, actions) plus
-- numbered immutable edits. The server never refuses an upload for
-- ownership reasons and never deletes a recording; the coach picks which
-- recording is the match result (`selected_recording`). See recording.rs.
--
-- The old `actions` table is kept as `legacy_actions` until the startup
-- migration (store::migrate_legacy) has turned every match's log into a
-- recording; it is emptied match by match and then only a shell remains.
-- The scout_* columns on matches stay as dead columns: SQLite cannot drop
-- a column that carries a foreign key, and rebuilding `matches` would
-- cascade into its dependents.

CREATE TABLE recordings (
    id           TEXT PRIMARY KEY,                       -- client-generated
    match_id     INTEGER NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
    team_id      INTEGER NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    user_id      INTEGER REFERENCES users(id) ON DELETE SET NULL,
    device_id    TEXT NOT NULL DEFAULT '',
    device_label TEXT NOT NULL DEFAULT '',
    origin_id    TEXT,                                   -- copied from (fork proof, no data dependency)
    origin_n     INTEGER,
    schema       INTEGER NOT NULL DEFAULT 1,
    base         TEXT NOT NULL,                          -- canonical JSON initial state
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    last_write   TEXT NOT NULL DEFAULT (datetime('now')) -- server receipt of the newest edit
);
CREATE INDEX idx_recordings_match ON recordings(match_id);

CREATE TABLE edits (
    recording_id TEXT NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
    n            INTEGER NOT NULL,                       -- 1, 2, 3 … dense per recording
    body         TEXT NOT NULL,                          -- canonical JSON
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (recording_id, n)
);

ALTER TABLE matches ADD COLUMN selected_recording TEXT REFERENCES recordings(id) ON DELETE SET NULL;
ALTER TABLE matches ADD COLUMN selection_rev INTEGER NOT NULL DEFAULT 0;
-- a match with recordings is archived instead of deleted, so late uploads keep their target
ALTER TABLE matches ADD COLUMN archived INTEGER NOT NULL DEFAULT 0;

ALTER TABLE actions RENAME TO legacy_actions;
