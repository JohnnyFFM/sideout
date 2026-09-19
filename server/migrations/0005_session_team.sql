-- The active team is per session (device), not per account: a coach with the
-- phone on the first team and the laptop on the second must not flip each
-- other. users.team_id stays as the default for new logins.
ALTER TABLE sessions ADD COLUMN team_id INTEGER REFERENCES teams(id);
