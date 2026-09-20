# Implement the new account / settings / team setup (SIDEOUT)

Implement the concept mocked in `mocks/einstellungen.html` (open it in a browser: `file:///C:/code/volleyball/mocks/einstellungen.html`, views `#konto`, `#app`, `#teams`; the avatar top right opens the menu). CONCEPT.md's screens table has a one-line summary. The mock is the visual spec: follow its order, wording and layout. UI language German, docs English.

Read first: `web/src/lib/components/Nav.svelte`, `web/src/routes/einstellungen/+page.svelte`, `web/src/routes/kader/+page.svelte`, `web/src/lib/stores.js` (me, refreshMe, switchTeam, reconnectSSE), `server/src/api/mod.rs`, `server/src/api/team.rs`, `server/src/api/auth_routes.rs`, `web/static/hilfe/index.html`.

## 1. Top bar: avatar menu instead of the Einstellungen link

- Replace the initials link in `Nav.svelte` with a round initials avatar button (34 px, accent-soft background, accent text, Barlow Condensed bold). Clicking opens a popover under the button, right aligned, floating on `document.body` with a high z-index so it sits above the phone tab bar. Pattern is kursbude's account popover (`../kursbude/ahub/web/src/shared/shell/topbar.js`, `renderAcct`): header, separator, rows, separator, sign out.
- Menu content: header with display name and "Rolle · aktives Team"; rows **Konto** (round initials glyph, hint "Name, Passwort"), **Einstellungen** (⚙, hint "Darstellung, App, Diagnose"), **Teams** (⚑, hint "N Teams · Stammdaten, Helfer, Team-Code"); then **Abmelden** in red. Current view row highlighted.
- Closes on outside click, Escape and navigation. Keep the theme toggle and the team switcher in the bar. Tab bar stays at four entries (Team, Spiele, Auswertung, Kader). No gear icon, no fifth tab.

## 2. Routes: three one-column pages

Replace `/einstellungen` with three routes (or one route with sub-paths, your call, but three URLs): `/konto`, `/einstellungen`, `/teams`. Each is a single column (`max-width` about 640 px for Konto and Einstellungen, 760 px for Teams), h1 plus a small muted subtitle.

### /konto ("dein Login, gilt für alle Teams")
- Card: avatar, display name, line "Trainer:in in N Teams · Co-Trainer:in in M" derived from `me.teams[].role`.
- Card **Name**: Login (read-only username), Anzeigename input (placeholder = username, hint "So siehst du für andere im Team aus. Leer lassen = Login."), Speichern. Empty display name saves as the username.
- Card **Passwort**: current, new, repeat; Passwort ändern. Validate with the existing `validate_credentials` rules.
- Dashed card **E-Mail** with chip "später" and the text from the mock. No inputs. Do not build registration by e-mail now.
- No session info row. Abmelden lives in the menu only; a footer line says so.

### /einstellungen ("dieses Gerät, diese App")
One panel with rows (label, muted description, control on the right): Darstellung (Dunkel / Hell segment, writes through `window.soTheme`, stays in sync with the bar toggle), Als App installieren (existing platform hint, chip "nicht installiert" / "installiert" via `display-mode: standalone`), Verbindung (online / Live-Stream / leader-follower text, green or red dot), Anleitung (button to `/hilfe/`), Diagnose as a collapsed `<details>` with the error count as a chip, the existing list and Kopieren / Leeren. Footer: version and "Open Source (MIT)".

### /teams ("ein Login, mehrere Teams · antippen zum Aufklappen")
- First panel **Team hinzufügen**: "Neues Team anlegen" + Anlegen, "Team beitreten" (code) + Beitreten, hint "Wer per Code beitritt, kann erst mal nur lesen. Trainer:innen befördern im Team zur Co-Trainer:in." On success refresh me, reconnect SSE, toast, stay on /teams with the new team expanded and active.
- Footer line under it: "Das aktive Team steht oben in der Leiste. Spiele, Auswertung und Kader gehören immer zum aktiven Team."
- Then one collapsible card per team from `me.teams`, **all collapsed by default**. Header: name with a caret, subline "Liga · Saison · Rolle · N Spielerinnen · M Personen", right side chip "aktiv" or a Wechseln button (switches the active team and expands that card). The active card has an accent border. Only the header toggles; Wechseln stops propagation.
- Expanded body, in this order:
  1. **Stammdaten**: Name, Kürzel (max 4), Liga, Saison, Speichern. Editable only when my role in that team is coach; otherwise a read-only line and subtitle "nur die Trainer:in ändert das".
  2. **Kader**: count of active players, small "N ehemalige", chips per position with counts (`chip pos-Z` etc., POS_NAME), button "Kader bearbeiten →" which switches the active team if needed and navigates to `/kader`.
  3. **Trainer:innen & Helfer** (subtitle "Rollen ändert nur die Trainer:in"): member list with initials, name, "(du)" marker, username; for a coach a role select and a remove button on every member except themselves, for everyone else a role chip. Below it the invite box (dashed): Team-Code large, text "Team-Code zum Beitreten. Wer beitritt, kann erst mal nur lesen. Zum Scouten oben auf Co-Trainer:in setzen.", Neu erzeugen. Invite box only for coaches. Non-coaches instead see "Du bist Co-Trainer:in: Rollen und Team-Code verwaltet <coach name>." plus a **Team verlassen** link (confirm first).
- Data: per-team counts and members need a per-team payload. Either extend `/me` (`teams[]` with `player_count`, `inactive_count`, `positions`, `member_count`, `join_code` for coaches, `members[]`) or add `GET /teams/{id}` used on expand. Prefer the lazy per-team GET so `/me` stays small.

## 3. Server changes

- **Join defaults to viewer.** Both join paths grant `assistant` today: `auth_routes::join` (register with code, users.role and memberships.role) and `team::join_team` (logged-in join). Change both to `viewer`. Promotion to assistant or coach happens via the existing `PATCH /team/members/{id}` by a coach. Update the audit text if it names the role.
- New endpoints: `PATCH /me` `{ display_name }` (empty → username), `POST /me/password` `{ current, new }` (verify current hash, reject same as username), `DELETE /teams/{id}/membership` (leave; refuse if I am the last coach of that team; if it was the active team switch to another or clear). Keep the existing team routes.
- Per-team detail if you chose the lazy GET: `GET /teams/{id}` returning stammdaten, my role, counts, members (with roles), join code only for coaches. Membership required.
- Update `server/tests/fixtures` and add API tests for: join gives viewer, display name empty falls back to username, password change requires the current one, leave refuses the last coach.

## 4. Help text

`web/static/hilfe/index.html` mentions "Über die Initialen oben rechts kommst du zu den Einstellungen", "Mitglieder", "Meine Teams" and says joiners become Co-Trainer:in. Update those passages: the avatar menu with Konto / Einstellungen / Teams, the Teams page structure, and that joiners start with Nur lesen and are promoted by the Trainer:in.

## 5. Rules

- Do not regress the live scouting screen or the offline queue; the settings work must not touch `live/` or `offline.js`.
- Tests run against port 8081 with their own database, never the 8080 instance; kill stale headless Edge before browser tests.
- Phone first (390 px, no horizontal scroll), then desktop. Dark and light theme.
- Commit in small steps: server role default + tests, new endpoints + tests, nav menu, the three pages, help text.
