-- SIDEOUT initial schema. Multi-tenant: team_id on every table, scoped in
-- every query. `actions` is the source of truth for a match: score, rotation,
-- serve, set boundaries and every statistic are replayed from it (engine.rs).
-- Mutable rows (matches) carry version/updated_at; writers do
-- UPDATE … WHERE id=? AND version=? (0 rows → 409 with the current row).

CREATE TABLE teams (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT NOT NULL,
    short      TEXT NOT NULL DEFAULT '',
    league     TEXT NOT NULL DEFAULT '',
    season     TEXT NOT NULL DEFAULT '',
    join_code  TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE users (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    team_id       INTEGER NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    username      TEXT NOT NULL UNIQUE,
    display_name  TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    -- coach: everything · assistant: scouts and edits matches · viewer: read only
    role          TEXT NOT NULL DEFAULT 'assistant' CHECK (role IN ('coach','assistant','viewer')),
    created_at    TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE sessions (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash   TEXT NOT NULL UNIQUE,
    device_label TEXT,
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    last_seen    TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Z Zuspiel · A Außen · M Mitte · D Diagonal · L Libero
CREATE TABLE players (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    team_id    INTEGER NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    number     INTEGER NOT NULL,
    name       TEXT NOT NULL,
    position   TEXT NOT NULL CHECK (position IN ('Z','A','M','D','L')),
    active     INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
-- a jersey number is unique among active players; a leaver frees it
CREATE UNIQUE INDEX idx_players_number ON players(team_id, number) WHERE active = 1;

CREATE TABLE matches (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    team_id     INTEGER NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    opponent    TEXT NOT NULL,
    date        TEXT NOT NULL,                 -- YYYY-MM-DD
    time        TEXT,                          -- HH:MM
    hall        TEXT NOT NULL DEFAULT '',
    home        INTEGER NOT NULL DEFAULT 1,
    first_serve TEXT NOT NULL DEFAULT 'us' CHECK (first_serve IN ('us','them')),
    status      TEXT NOT NULL DEFAULT 'planned' CHECK (status IN ('planned','live','done')),
    notes       TEXT NOT NULL DEFAULT '',
    version     INTEGER NOT NULL DEFAULT 1,
    created_by  INTEGER REFERENCES users(id) ON DELETE SET NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_matches_team_date ON matches(team_id, date);

-- starting six per set (position I..VI = pos1..pos6, I serves) + libero.
-- A set without a row inherits the previous set's lineup.
CREATE TABLE lineups (
    match_id  INTEGER NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
    set_no    INTEGER NOT NULL,
    pos1 INTEGER NOT NULL REFERENCES players(id),
    pos2 INTEGER NOT NULL REFERENCES players(id),
    pos3 INTEGER NOT NULL REFERENCES players(id),
    pos4 INTEGER NOT NULL REFERENCES players(id),
    pos5 INTEGER NOT NULL REFERENCES players(id),
    pos6 INTEGER NOT NULL REFERENCES players(id),
    libero_id INTEGER REFERENCES players(id),
    PRIMARY KEY (match_id, set_no)
);

-- the scout log. seq is client-assigned and dense per match, which makes a
-- retried POST idempotent (UNIQUE) and keeps replay order explicit.
-- skill S R E A B D carry a grade (# + ! - / =) and a player;
-- 'opp' carries grade '#' (opponent point) or '=' (opponent error);
-- 'sub' carries sub_out / sub_in.
CREATE TABLE actions (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    match_id   INTEGER NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
    seq        INTEGER NOT NULL,
    set_no     INTEGER NOT NULL,
    skill      TEXT NOT NULL CHECK (skill IN ('S','R','E','A','B','D','opp','sub')),
    grade      TEXT CHECK (grade IN ('#','+','!','-','/','=')),
    player_id  INTEGER REFERENCES players(id),
    sub_out    INTEGER REFERENCES players(id),
    sub_in     INTEGER REFERENCES players(id),
    created_by INTEGER REFERENCES users(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (match_id, seq)
);

CREATE TABLE audit_log (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    team_id    INTEGER NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    entity     TEXT NOT NULL,
    entity_id  INTEGER NOT NULL,
    action     TEXT NOT NULL,
    detail     TEXT NOT NULL DEFAULT '',
    user_id    INTEGER REFERENCES users(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_audit_team ON audit_log(team_id, id);
