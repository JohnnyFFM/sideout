# Sideout — live volleyball scouting, open source

A coach scouts their own team from the bench: tap the player, tap the
action, done. Score, rotation, serve, side-out and every statistic are
derived from that log, live, per set, per match, per season. Phone first,
works offline in the hall, installs as a PWA, runs on any small VPS.

Stats follow the Data Volley convention (six skills × six grades), so the
numbers are the ones coaches and analysts already know: hitting
efficiency, pass rating 0–3, side-out and break percentages, side-out per
rotation. `CONCEPT.md` explains the standard and every derived number.

## Stack

Rust + Axum + SQLite (WAL, one writer) serving a SvelteKit static PWA.
One binary, one database file, one Docker image. Caddy in front on the VPS.

```
server/     Rust API + static serving + the match engine (engine.rs)
web/        SvelteKit SPA; lib/engine.js is the client twin of engine.rs
mocks/      phase-1 HTML mocks, still the visual spec
deploy/     Caddy ingress + live/test compose stacks
```

The engine exists twice on purpose: the browser replays the action log
locally so every tap renders instantly and offline, the server replays it
for stats, exports and season totals. `web/scripts/fixture.mjs` generates
a 600-action match from the JS engine and the Rust test
`parity_with_js_fixture` proves both agree.

## Run it locally

```
cd server && cargo run                  # http://localhost:8080, creates ./server/data/sideout.db
cd web && npm install && npm run dev    # hot reload on :5173, proxies /api → 8080
```

Or the production shape: `cd web && npm run build`, then `cargo run` serves
the built app from `web/build`. Open it, choose "Team anlegen", add the
roster under Kader, create a match with a starting six, tap.

Tests: `cd server && cargo test` (regenerate the parity fixture with
`cd web && npm run fixture` after changing the engine).

## Run it on a server

```
docker build -t sideout .
docker run -p 8080:8080 -v sideout-data:/data sideout
```

For the VPS pattern (Caddy ingress, live + test stacks, GitHub Actions on
push to `master` / `dev`), see `deploy/` and `.github/workflows/deploy.yml`.
Set the secrets `VPS_HOST`, `VPS_USER`, `VPS_SSH_KEY` in a GitHub
environment called `vps`; without them the workflow only runs CI.

Env vars: `SO_DB` (default `./data/sideout.db`), `SO_ADDR` (default
`127.0.0.1:8080`; `0.0.0.0:8080` in the image), `SO_STATIC` (default
`../web/build`), `SO_DATA` (default `./data`), `SO_SIGNUP` (`closed`
disables self-registration and join-by-code), `SO_BASE` (path prefix such
as `/sideout` when the app shares a host with other apps; it is baked into
the web build, so pass it as the Docker build arg `SO_BASE` and set the
same value at runtime; the workflow reads it from the repo variable
`SO_BASE`). For a local prefixed build next to the plain one, set
`SO_OUTDIR` (e.g. `SO_BASE=/sideout SO_OUTDIR=build-base npm run build`) and
point `SO_STATIC` at that directory.

A gated instance seeds accounts with the CLI:

```
docker exec -e SO_PASSWORD=... sideout-live /app/server team-add "TSV Eintracht"
docker exec -e SO_PASSWORD=... sideout-live /app/server user-add 1 jonas Jonas coach
docker exec sideout-live /app/server user-list
```

## How scouting works

- **Roles**: coach (everything), assistant (scouts, edits matches), viewer.
  A team has a join code; whoever joins becomes an assistant. One account
  can belong to several teams with a role per team (a coach with the first
  and the second team in two leagues); the active team is switched in the
  top bar, and everything else is scoped to it.
- **Court**: own half from behind the baseline, front row IV III II at the
  net, back row V VI I. The libero stands in for the back-row middle on
  V and VI, and on I while the opponent serves; she never serves herself.
- **Pad**: rows are the skills Aufschlag, Annahme, Zuspiel, Angriff, Block,
  Abwehr; columns the grades `#` `+` `!` `-` `/` `=`. Point and error grades
  end the rally, everything else follows: score, rotation on a won
  reception rally, serve, set end at 25 (15 in the fifth) with two clear,
  match end at three sets.
- **Speed**: the expected next skill is highlighted from the rally phase
  and so is the likely player (the server on I, the setter for a set);
  a serve or set tapped without a selection books itself to her. Two big
  buttons cover rallies the opponent ends, undo pops the last action.
  Keyboard on a laptop: 1–6 select positions, Ctrl+Z undoes.
- **Catch-up**: four dashed buttons under the score for the coach who
  missed a few rallies: +1 for either side (score only, no rally, rotation
  or serve change, excluded from side-out stats), rotate one position, set
  who serves. Each is a logged action (`adj`, `rot`, `srv`) and undoable.
- **Bench**: a strip of chips under the court for everyone not on it.
  While the libero sits she is the first chip; while she stands in for
  someone, that player is the first chip. Drag a chip onto a court slot
  (pointer events, works with touch) or tap the chip and then the slot. A
  bench player replaces the slot's player (substitution); the libero
  dropped on a back-row slot (V, VI, or I while receiving) stands in for
  that player as long as she is in the back row and not serving (a `lib` action in the log; without one she automatically covers
  the back-row middle); dropping the replaced player back on her own card
  sends the libero out.
- **Offline**: the live page keeps the match and an op queue in
  localStorage. Taps render from the local replay and are sent when the
  connection is back; retries are idempotent (`seq`), a conflict with
  another device reloads the match.
- **Evaluation**: box score per player and set, score flow, side-out by
  rotation, points by source, quality stacks per skill, CSV export.
  Season view with per-player totals and trends.

## API

All under `/api`, JSON, cookie session, `X-Requested-By` header on every
non-GET. Errors are `{"error": "..."}`, optimistic-locking conflicts are
409 with the current row.

```
POST /auth/register-team · /auth/join · /auth/login · /auth/logout   GET /config · /me
PATCH /team · POST /team/rotate-code · PATCH|DELETE /team/members/{id}
POST /teams {name} · POST /teams/join {code} · POST /teams/switch {team_id}
GET|POST /players · PATCH|DELETE /players/{id}
GET|POST /matches · GET|PATCH|DELETE /matches/{id}
PUT /matches/{id}/lineups/{set}          {pos:[6 player ids], libero}
POST /matches/{id}/actions               {seq, skill, grade?, player_id?, sub_out?, sub_in?} → {action, state}
DELETE /matches/{id}/actions/last        → {removed, state}
GET /matches/{id}/state · /matches/{id}/stats?set= · /matches/{id}/export.csv
GET /season/stats
GET /events                              SSE, one notification per committed mutation
```

## Status and roadmap

Working: teams, roles, roster, matches with per-set lineups, live scouting
with offline queue, undo, substitutions, live evaluation, box score,
charts, CSV export, season totals, PWA install, SSE live updates between
devices, Docker + CI.

Next: player pages across the season, `.dvw` export for Data Volley and
the openvolley toolchain, attack directions on the court, a read-only
live link for parents, a second scouter on a second phone.

## License

MIT. Every coach deserves this.
