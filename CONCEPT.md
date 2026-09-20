# SIDEOUT — live volleyball scouting for team coaches

> Working title. Sibling of UnserPlan (`../famcal`): same stack, same
> deployment pattern, same "mocks are the visual spec" convention.
> UI language German, docs English.

## The job

A coach on the bench scouts their own team during a match. Every contact
gets one tap-pair (player, then action+grade). The app derives score,
rotation, serve/receive, side-out and every statistic from that log, live,
per set and per match, and later per season. No second person, no laptop,
no typing. Phone in one hand, sight line on the court.

Not in scope (v1): scouting the opponent, video, ball trajectories,
attack directions, setter-distribution heat maps. All of these fit the
data model later (they are extra attributes on an action).

## Is there a professional standard? Yes — Data Volley

The pro/college world runs on **Data Volley** (DataProject, Italy). Every
FIVB/CEV/Bundesliga/NCAA scout writes the same code per contact:

```
*12 A H #      team · player · skill · (type) · grade
```

Six skills and six grades. This is the vocabulary SIDEOUT uses internally,
one to one, so exports and comparisons stay compatible:

| Code | Skill | German |
|---|---|---|
| S | Serve | Aufschlag |
| R | Reception | Annahme |
| E | Set | Zuspiel |
| A | Attack | Angriff |
| B | Block | Block |
| D | Dig | Abwehr |

| Grade | Meaning | Rally |
|---|---|---|
| `#` | point / perfect (ace, kill, stuff block, perfect pass) | S, A, B end the rally for us |
| `+` | positive | continues |
| `!` | neutral | continues |
| `-` | negative (poor pass, easily dug attack) | continues |
| `/` | "half": attack blocked, reception overpass | A ends the rally for them |
| `=` | error | ends the rally for them |

Everything a coach knows from box scores is a projection of this log:

- **Attack**: attempts, kills, errors, blocked; *hitting efficiency*
  (K − E − blocked) / attempts, the number everyone quotes (".300" is very good
  at club level); kill % = K / attempts.
- **Serve**: attempts, aces, errors; break % (rallies won on own serve).
- **Reception**: attempts, perfect %, positive % (# and +), errors, and the
  **0–3 pass rating** clubs use (perfect 3, good 2, OK 1, negative/overpass/error 0).
  Data Volley's finer six steps are kept, the 0–3 is derived.
- **Block**: kills (stuff), touches, errors. (Assisted blocks / 0.5 points
  as in NCAA box scores can be added later by tagging two blockers.)
- **Dig**: successful digs, errors.
- **Set**: attempts, assists (a set immediately followed by a kill), errors.
- **Team**: side-out % (rallies won when receiving), break % (won when
  serving), side-out per rotation, points by source (attack / serve / block /
  opponent error), points lost by cause, score-difference flow per set.

FIVB's own statistics (VIS) use fewer categories (e.g. attack: kills,
faults, shots); NCAA box scores use K/E/TA, SA/SE, RE, DIG, BS/BA/BE, AST.
Both are subsets of the Data-Volley log, so the export can serve either.

## The capture UX (the whole product lives here)

Two taps per contact, nothing hidden, nothing modal:

1. **Tap the player** on the court diagram (own half, seen from behind the
   own baseline: front row IV III II at the net, back row V VI I). The
   libero automatically stands in for the back-row middle on V/VI; on I the
   middle serves, so she is shown there.
2. **Tap one cell of the pad**: rows are the six skills, columns the six
   grades; each cell shows the code and the skill-specific word
   (Ass / Punkt / Overpass / Geblockt …). Cells that do not exist for a skill
   are blank (no "/" for serve, set, block, dig).

Speed helpers, all optional:

- The app predicts the next skill from the rally phase (serve → block/dig →
  set → attack → …; reception → set → attack) and highlights that row.
- When we serve, the server is position I; tapping a serve cell without a
  selected player books it to her.
- **Fehler Gegner** and **Punkt Gegner** as two large buttons for rallies the
  opponent ends.
- **Rückgängig** pops the last action. Because everything is replayed from
  the log, score, rotation and serve fall back automatically.
- Sub sheet (out / in), keyboard 1–6 on desktop for positions, Ctrl+Z.

What the log gives us for free: side-out won on reception → rotate; set ends
at 25 (15 in the fifth) with two clear; next set the serve alternates;
match ends at three sets.

## Screens (see `mocks/`)

| Mock | Purpose | Phone | Tablet / desktop |
|---|---|---|---|
| `live.html` | capture | score · court · pad · undo, one column | court+score left, pad centre, live table + log right |
| `auswertung.html` | evaluation | tiles, box score (horizontal scroll), charts stacked | full box score, 2-col charts |
| `spiel.html` | match setup | form, lineup court, roster | two columns |
| `index.html` | team home | live card, season tiles, matches, top lists | three columns |
| `einstellungen.html` | account menu + settings | avatar → menu (Konto · Einstellungen · Teams · Abmelden); Teams as 5th tab; every view one column | same, narrow column |

The live screen is designed for a 390 px phone first; tap targets are
52 px minimum, contrast tuned for a bright hall, and the pad never
re-flows between taps.

## Data model (SQLite, same conventions as UnserPlan)

```
teams(id, name, short, league, season, join_code, created_at)
users(id, team_id, username, display_name, password_hash, role CHECK(coach|assistant|viewer), …)
players(id, team_id, number, name, position CHECK(Z|A|M|D|L), active, created_at)
matches(id, team_id, opponent, date, time, hall, home, first_serve CHECK(us|them),
        status CHECK(planned|live|done), notes, version, …)
match_players(match_id, player_id)                       -- squad for the day
lineups(match_id, set_no, pos1..pos6 player_id, libero_id)  -- per set (default: previous set)
actions(id, match_id, seq INTEGER, set_no, skill CHECK(S|R|E|A|B|D|opp|sub),
        grade CHECK(#|+|!|-|/|=), player_id NULL, sub_out NULL, sub_in NULL,
        created_by, created_at, deleted)                 -- append-only, undo = soft delete of last
audit_log(...)
```

Rules:

- `actions` is the source of truth. Score, rotation, serve, rally index and
  set boundaries are **derived** on read (`replay()` — see `mocks/data.js`
  for the reference implementation, to be ported 1:1 to Rust).
- Derived stats are cached per (match, set) on write for the evaluation
  endpoints; a cheap recompute on demand is fine for club-size data
  (a match is ~600 actions).
- Optimistic locking on `matches` and `lineups` only; `actions` is
  append-only with a per-match `seq` so two devices can never collide
  (a second scouter is a later feature — first version is one device).

## API sketch (`/api`, JSON, same error shape as UnserPlan)

```
POST /api/matches                      create
PATCH /api/matches/{id}                {version, …}
PUT  /api/matches/{id}/lineup/{set}    {pos:[6 ids], libero}
POST /api/matches/{id}/actions         {skill, grade, player_id?} → {action, state}
DELETE /api/matches/{id}/actions/last  → {state}           (undo)
GET  /api/matches/{id}/state           {set, us, them, sets, lineup, serving, rally, court}
GET  /api/matches/{id}/stats?set=      {players:[…], team:{…}, rallies:[…]}
GET  /api/matches/{id}/export.csv      Data-Volley-like flat export
GET  /api/season/stats                 per player / team, all done matches
GET  /api/events                       SSE: {entity:'match', id, seq}  → a second screen refetches /state
```

`POST …/actions` returns the fresh derived state so the phone never
computes anything itself except optimistic display.

## Offline first (this one is not optional)

Sports halls have bad reception. The PWA keeps the match log in IndexedDB,
appends locally, renders from the local replay, and syncs the queue when a
connection is back (`seq` makes the replay idempotent). Service worker
caches the app shell. Install prompt on iOS/Android; "Add to home screen"
gets the full-screen, no-browser-chrome mode the pad needs.

## Stack (UnserPlan pattern)

- **Server**: Rust, Axum 0.8, SQLx 0.8, SQLite WAL (1 writer / 4 readers),
  argon2id sessions, SSE event bus, static serving of the SPA, CLI seeding.
- **Web**: SvelteKit 2 / Svelte 5, `adapter-static`, design system ported
  from `mocks/style.css`, PWA (manifest + service worker + IndexedDB queue).
- **Deploy**: Dockerfile (node build → rust build → debian-slim), Caddy on
  the Hetzner box, `live`/`test` compose stacks, GitHub Actions on push.
- **Multi-tenant** from day one (team = tenant), gated signup like UnserPlan.

## Roadmap

1. **Mocks** (`mocks/`) — capture flow, evaluation, setup, team home. ✔
2. **App** (`server/`, `web/`): schema, auth, roster, matches with per-set
   lineups, actions + replay in Rust and JS (parity-tested), live screen
   with offline op queue, undo, substitutions, evaluation, CSV export,
   season totals, PWA, SSE, Docker + CI. ✔ (2026-09-19)
3. **Season**: player pages, trends across matches, comparison of two
   matches.
4. Later: attack direction / zones on the court, opponent scouting,
   second scouter (two phones, one match), share a read-only live link with
   parents/players, Data Volley `.dvw` export for the openvolley toolchain.
5. Offline mode ✔ (2026-09-20): Spiele prefetches running and planned
   matches ("offline bereit"), the identity is remembered for offline
   starts, the evaluation reads the local log, and every action carries a
   client id (`cid`) so the queue reconciles against the server after lost
   answers (no double counts, no double undo). Still open: creating a match
   offline.

## Decisions worth remembering

- `actions.seq` is client-assigned and dense per match. It makes retries
  idempotent (same seq + same payload → the stored row), lets the server
  reject a stale writer (409 with `last_seq`), and keeps replay order
  explicit without timestamps.
- Undo is a hard delete of the last action, audited. There is no edit of
  an action in the middle of the log; the coach undoes back to it.
- Set boundaries are never stored. The engine derives them, so a wrong
  tap near 25 is fixed by undo like any other tap.
- Player positions (Z A M D L) only drive the libero display and the
  lineup plausibility check, never the statistics.
