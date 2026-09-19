-- A user can belong to several teams with a role per team. users.team_id
-- becomes the currently selected team; users.role stays as the default
-- role for CLI-created accounts but authorization reads memberships.

CREATE TABLE memberships (
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    team_id    INTEGER NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    role       TEXT NOT NULL DEFAULT 'assistant' CHECK (role IN ('coach','assistant','viewer')),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (user_id, team_id)
);
CREATE INDEX idx_memberships_team ON memberships(team_id);

INSERT INTO memberships (user_id, team_id, role) SELECT id, team_id, role FROM users;
