-- Catch-up actions for the coach who missed a few rallies:
--   adj  grade '#' = +1 for us, '=' = +1 for them (score only, no rotation
--        or serve change, not a rally for side-out statistics)
--   rot  rotate our lineup one position
--   srv  grade '#' = we serve, '=' = they serve
-- SQLite cannot alter a CHECK constraint, so the table is rebuilt.

CREATE TABLE actions_new (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    match_id   INTEGER NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
    seq        INTEGER NOT NULL,
    set_no     INTEGER NOT NULL,
    skill      TEXT NOT NULL CHECK (skill IN ('S','R','E','A','B','D','opp','sub','lib','adj','rot','srv')),
    grade      TEXT CHECK (grade IN ('#','+','!','-','/','=')),
    player_id  INTEGER REFERENCES players(id),
    sub_out    INTEGER REFERENCES players(id),
    sub_in     INTEGER REFERENCES players(id),
    created_by INTEGER REFERENCES users(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (match_id, seq)
);
INSERT INTO actions_new (id, match_id, seq, set_no, skill, grade, player_id, sub_out, sub_in, created_by, created_at)
    SELECT id, match_id, seq, set_no, skill, grade, player_id, sub_out, sub_in, created_by, created_at FROM actions;
DROP TABLE actions;
ALTER TABLE actions_new RENAME TO actions;
