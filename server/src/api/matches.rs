//! Matches: the frame (opponent, date, …), the planning (first serve and
//! lineups before scouting starts), and every result view — state, stats,
//! export, season — all read from the match's selected recording.

use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;

use crate::auth::{CurrentUser, Role};
use crate::engine;
use crate::error::{ApiError, ApiResult};
use crate::events::EventMsg;
use crate::recording::stat_players;
use crate::state::AppState;
use crate::store::{
    action_json, audit, audit_conn, fetch_match_row, fetch_match_row_conn, lineups_json, load_planning_conn, load_players,
    match_json, planning_snapshot_conn, mark_superseded, rec_meta_json, recordings_of_conn, selected_snapshot, selected_snapshot_conn,
    snapshot_of_conn, state_summary,
};

pub(super) fn publish(state: &AppState, user: &CurrentUser, entity: &str, id: i64, version: i64, action: &str) {
    state.events.publish(EventMsg {
        team_id: user.team_id,
        entity: entity.into(),
        id,
        version,
        action: action.into(),
        actor: user.display_name.clone(),
    });
}

pub async fn list(State(state): State<AppState>, user: CurrentUser) -> ApiResult<Json<Value>> {
    let rows = sqlx::query("SELECT * FROM matches WHERE team_id = ? AND archived = 0 ORDER BY date DESC, id DESC")
        .bind(user.team_id)
        .fetch_all(&state.db)
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    let mut conn = state.db.acquire().await?;
    for r in &rows {
        let mut m = match_json(r);
        let (_, snap) = selected_snapshot_conn(&mut conn, r).await?;
        m["state"] = serde_json::to_value(engine::replay(&snap.cfg, &snap.actions)).unwrap_or(Value::Null);
        let mut recs = recordings_of_conn(&mut conn, r.get("id")).await?;
        mark_superseded(&mut recs, r.get::<Option<String>, _>("selected_recording").as_deref());
        m["recordings"] = json!(recs.iter().filter(|x| !x.superseded).count());
        // who is scouting right now (a recording with a write in the last minutes)
        m["scouting"] = recs
            .iter()
            .filter(|x| x.active)
            .max_by(|a, b| a.last_write.cmp(&b.last_write))
            .map(|x| json!({ "actor": x.user_name, "device": x.device_label, "since": x.created_at }))
            .unwrap_or(Value::Null);
        out.push(m);
    }
    Ok(Json(json!({ "matches": out })))
}

#[derive(Deserialize)]
pub struct LineupBody {
    pub pos: Vec<i64>,
    pub libero: Option<i64>,
}

#[derive(Deserialize)]
pub struct NewMatch {
    pub opponent: String,
    pub date: String,
    pub time: Option<String>,
    #[serde(default)]
    pub hall: String,
    #[serde(default = "default_true")]
    pub home: bool,
    #[serde(default = "default_us")]
    pub first_serve: String,
    #[serde(default)]
    pub notes: String,
    pub lineup: Option<LineupBody>,
}
fn default_true() -> bool { true }
fn default_us() -> String { "us".into() }

fn validate_match(opponent: &str, date: &str, first_serve: &str) -> ApiResult<()> {
    if opponent.trim().is_empty() {
        return Err(ApiError::BadRequest("Gegner fehlt".into()));
    }
    if chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
        return Err(ApiError::BadRequest("Datum ungültig".into()));
    }
    if !matches!(first_serve, "us" | "them") {
        return Err(ApiError::BadRequest("first_serve muss us oder them sein".into()));
    }
    Ok(())
}

async fn validate_lineup(state: &AppState, team_id: i64, l: &LineupBody) -> ApiResult<()> {
    if l.pos.len() != 6 {
        return Err(ApiError::BadRequest("Aufstellung braucht genau 6 Positionen".into()));
    }
    let mut ids = l.pos.clone();
    if let Some(lib) = l.libero { ids.push(lib); }
    let mut uniq = ids.clone();
    uniq.sort();
    uniq.dedup();
    if uniq.len() != ids.len() {
        return Err(ApiError::BadRequest("Eine Spielerin steht doppelt in der Aufstellung".into()));
    }
    let n: i64 = sqlx::query(&format!(
        "SELECT count(*) AS n FROM players WHERE team_id = ? AND id IN ({})",
        ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",")
    ))
    .bind(team_id)
    .fetch_one(&state.db)
    .await?
    .get("n");
    if n as usize != ids.len() {
        return Err(ApiError::BadRequest("Unbekannte Spielerin in der Aufstellung".into()));
    }
    Ok(())
}

async fn write_lineup_conn(conn: &mut sqlx::SqliteConnection, match_id: i64, set: i64, l: &LineupBody) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO lineups (match_id, set_no, pos1, pos2, pos3, pos4, pos5, pos6, libero_id)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(match_id, set_no) DO UPDATE SET pos1=excluded.pos1, pos2=excluded.pos2, pos3=excluded.pos3,
           pos4=excluded.pos4, pos5=excluded.pos5, pos6=excluded.pos6, libero_id=excluded.libero_id",
    )
    .bind(match_id).bind(set)
    .bind(l.pos[0]).bind(l.pos[1]).bind(l.pos[2]).bind(l.pos[3]).bind(l.pos[4]).bind(l.pos[5])
    .bind(l.libero)
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(body): Json<NewMatch>,
) -> ApiResult<Json<Value>> {
    user.require(Role::Assistant)?;
    validate_match(&body.opponent, &body.date, &body.first_serve)?;
    if let Some(l) = &body.lineup {
        validate_lineup(&state, user.team_id, l).await?;
    }
    let mut tx = state.dbw.begin().await?;
    let res = sqlx::query(
        "INSERT INTO matches (team_id, opponent, date, time, hall, home, first_serve, notes, created_by)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(user.team_id)
    .bind(body.opponent.trim())
    .bind(&body.date)
    .bind(body.time.as_deref().filter(|t| !t.is_empty()))
    .bind(body.hall.trim())
    .bind(body.home as i64)
    .bind(&body.first_serve)
    .bind(body.notes.trim())
    .bind(user.id)
    .execute(&mut *tx)
    .await?;
    let id = res.last_insert_rowid();
    if let Some(l) = &body.lineup {
        write_lineup_conn(&mut tx, id, 1, l).await?;
    }
    audit_conn(&mut tx, user.team_id, "match", id, "created", body.opponent.trim(), Some(user.id)).await?;
    tx.commit().await?;
    publish(&state, &user, "match", id, 1, "created");
    get_one(State(state), user, Path(id)).await
}

pub async fn get_one(State(state): State<AppState>, user: CurrentUser, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    Ok(Json(match_payload(&state, &user, id).await?))
}

/// the full match of the caller's active team (see `match_payload_row`)
pub async fn match_payload(state: &AppState, user: &CurrentUser, id: i64) -> ApiResult<Value> {
    let row = fetch_match_row(state, user.team_id, id).await?;
    match_payload_row(state, &row).await
}

/// The full match: frame, planning, roster, every recording's summary, and
/// the result — first serve, lineups, roster and actions of the selected
/// recording (the planning while nothing is recorded) with its replayed state.
pub async fn match_payload_row(state: &AppState, row: &sqlx::sqlite::SqliteRow) -> ApiResult<Value> {
    let id: i64 = row.get("id");
    let team_id: i64 = row.get("team_id");
    let mut conn = state.db.acquire().await?;
    let planning = load_planning_conn(&mut conn, id, &row.get::<String, _>("first_serve")).await?;
    let players = sqlx::query("SELECT * FROM players WHERE team_id = ? ORDER BY number")
        .bind(team_id)
        .fetch_all(&mut *conn)
        .await?;
    let (sel, snap) = selected_snapshot_conn(&mut conn, row).await?;
    let mut recs = vec![];
    let mut all = recordings_of_conn(&mut conn, id).await?;
    mark_superseded(&mut all, sel.as_deref());
    for r in all {
        let mut j = rec_meta_json(&r);
        if let Some(s) = snapshot_of_conn(&mut conn, &r.id).await? {
            j["state"] = state_summary(&engine::replay(&s.cfg, &s.actions));
        }
        j["selected"] = json!(sel.as_deref() == Some(r.id.as_str()));
        recs.push(j);
    }
    let mut m = match_json(row);
    m["planning"] = json!({ "first_serve": row.get::<String, _>("first_serve"), "lineups": lineups_json(&planning) });
    m["players"] = json!(players.iter().map(crate::store::player_json).collect::<Vec<_>>());
    m["recordings"] = json!(recs);
    m["first_serve"] = json!(if snap.cfg.first_serve_us { "us" } else { "them" });
    m["lineups"] = lineups_json(&snap.cfg);
    m["roster"] = json!(snap.roster);
    m["actions"] = json!(snap.actions.iter().map(action_json).collect::<Vec<_>>());
    m["state"] = serde_json::to_value(engine::replay(&snap.cfg, &snap.actions)).unwrap_or(Value::Null);
    Ok(m)
}

#[derive(Deserialize)]
pub struct PatchMatch {
    pub version: i64,
    pub opponent: Option<String>,
    pub date: Option<String>,
    pub time: Option<String>,
    pub hall: Option<String>,
    pub home: Option<bool>,
    /// planning only: a started recording keeps its own first serve
    pub first_serve: Option<String>,
    pub notes: Option<String>,
}

pub async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(body): Json<PatchMatch>,
) -> ApiResult<Json<Value>> {
    user.require(Role::Assistant)?;
    let mut tx = state.dbw.begin().await?;
    let cur = fetch_match_row_conn(&mut tx, user.team_id, id).await?;
    let opponent = body.opponent.clone().unwrap_or(cur.get("opponent"));
    let date = body.date.clone().unwrap_or(cur.get("date"));
    let first_serve = body.first_serve.clone().unwrap_or(cur.get("first_serve"));
    validate_match(&opponent, &date, &first_serve)?;
    let res = sqlx::query(
        "UPDATE matches SET opponent = ?, date = ?, time = ?, hall = ?, home = ?, first_serve = ?, notes = ?,
         version = version + 1, updated_at = datetime('now') WHERE id = ? AND version = ?",
    )
    .bind(opponent.trim())
    .bind(&date)
    .bind(body.time.clone().or(cur.get("time")).filter(|t| !t.is_empty()))
    .bind(body.hall.clone().unwrap_or(cur.get("hall")).trim())
    .bind(body.home.map(|b| b as i64).unwrap_or(cur.get("home")))
    .bind(&first_serve)
    .bind(body.notes.clone().unwrap_or(cur.get("notes")).trim())
    .bind(id)
    .bind(body.version)
    .execute(&mut *tx)
    .await?;
    if res.rows_affected() == 0 {
        let current = fetch_match_row_conn(&mut tx, user.team_id, id).await?;
        return Err(ApiError::Conflict(match_json(&current)));
    }
    audit_conn(&mut tx, user.team_id, "match", id, "updated", "", Some(user.id)).await?;
    tx.commit().await?;
    publish(&state, &user, "match", id, body.version + 1, "updated");
    get_one(State(state), user, Path(id)).await
}

/// A match is archived, never deleted: hidden from the lists, still a target
/// for late uploads. The server cannot know whether a phone has recorded the
/// match offline, so even a "planned" match keeps its id.
pub async fn remove(State(state): State<AppState>, user: CurrentUser, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    user.require(Role::Coach)?;
    let mut tx = state.dbw.begin().await?;
    fetch_match_row_conn(&mut tx, user.team_id, id).await?;
    sqlx::query("UPDATE matches SET archived = 1, version = version + 1, updated_at = datetime('now') WHERE id = ?").bind(id).execute(&mut *tx).await?;
    audit_conn(&mut tx, user.team_id, "match", id, "archived", "", Some(user.id)).await?;
    tx.commit().await?;
    publish(&state, &user, "match", id, 0, "deleted");
    Ok(Json(json!({ "ok": true, "archived": true })))
}

/// PUT /matches/{id}/lineups/{set} — the planning lineup. A recording that
/// has started keeps its own lineups (they are edits of the recording).
pub async fn put_lineup(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((id, set)): Path<(i64, i64)>,
    Json(body): Json<LineupBody>,
) -> ApiResult<Json<Value>> {
    user.require(Role::Assistant)?;
    if !(1..=5).contains(&set) {
        return Err(ApiError::BadRequest("Satz muss 1–5 sein".into()));
    }
    validate_lineup(&state, user.team_id, &body).await?;
    let mut tx = state.dbw.begin().await?;
    let row = fetch_match_row_conn(&mut tx, user.team_id, id).await?;
    write_lineup_conn(&mut tx, id, set, &body).await?;
    sqlx::query("UPDATE matches SET version = version + 1, updated_at = datetime('now') WHERE id = ?").bind(id).execute(&mut *tx).await?;
    audit_conn(&mut tx, user.team_id, "match", id, "lineup", &format!("Satz {set}"), Some(user.id)).await?;
    tx.commit().await?;
    publish(&state, &user, "match", id, row.get::<i64, _>("version") + 1, "lineup");
    get_one(State(state), user, Path(id)).await
}

/// the old protocol's write routes: see `ApiError::Gone`
pub async fn gone() -> ApiResult<Json<Value>> {
    Err(ApiError::Gone)
}

pub async fn state(State(state): State<AppState>, user: CurrentUser, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let row = fetch_match_row(&state, user.team_id, id).await?;
    let (_, snap) = selected_snapshot(&state, &row).await?;
    Ok(Json(serde_json::to_value(engine::replay(&snap.cfg, &snap.actions)).unwrap_or(Value::Null)))
}

#[derive(Deserialize)]
pub struct StatsQuery {
    pub set: Option<i64>,
}

pub async fn stats(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Query(q): Query<StatsQuery>,
) -> ApiResult<Json<Value>> {
    let row = fetch_match_row(&state, user.team_id, id).await?;
    let (_, snap) = selected_snapshot(&state, &row).await?;
    let players = stat_players(&load_players(&state, user.team_id).await?, &snap);
    Ok(Json(engine::stats(&snap.cfg, &players, &snap.actions, q.set.filter(|s| *s > 0))))
}

pub async fn export_csv(State(state): State<AppState>, user: CurrentUser, Path(id): Path<i64>) -> ApiResult<impl IntoResponse> {
    let row = fetch_match_row(&state, user.team_id, id).await?;
    let (_, snap) = selected_snapshot(&state, &row).await?;
    let players = stat_players(&load_players(&state, user.team_id).await?, &snap);
    let csv = engine::export_csv(&snap.cfg, &players, &snap.actions);
    let name = format!("sideout-{}-{}.csv", row.get::<String, _>("date"), row.get::<String, _>("opponent").replace(' ', "_"));
    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\"")),
        ],
        csv,
    ))
}

/// Season view: one line per scouted match plus per-player totals across
/// all of them. Everything derives from the selected recordings.
pub async fn season(State(state): State<AppState>, user: CurrentUser) -> ApiResult<Json<Value>> {
    let rows = sqlx::query("SELECT * FROM matches WHERE team_id = ? AND archived = 0 AND status IN ('live','done') ORDER BY date, id")
        .bind(user.team_id)
        .fetch_all(&state.db)
        .await?;
    let team_players = load_players(&state, user.team_id).await?;
    let mut matches = vec![];
    let mut totals: std::collections::BTreeMap<i64, Value> = Default::default();
    let mut team = json!({"so_won": 0, "so_n": 0, "brk_won": 0, "brk_n": 0, "a_k": 0, "a_e": 0, "a_n": 0, "r_sum": 0, "r_n": 0, "won": 0, "lost": 0, "sets_won": 0, "sets_lost": 0});
    let add = |v: &mut Value, k: &str, d: i64| { v[k] = json!(v[k].as_i64().unwrap_or(0) + d); };
    let mut conn = state.db.acquire().await?;
    for r in &rows {
        let (_, snap) = selected_snapshot_conn(&mut conn, r).await?;
        let players = stat_players(&team_players, &snap);
        let s = engine::stats(&snap.cfg, &players, &snap.actions, None);
        let st = &s["state"];
        let t = &s["team"];
        let mut a = (0i64, 0i64, 0i64);
        let mut rr = (0i64, 0i64);
        for p in s["players"].as_array().unwrap() {
            let pid = p["id"].as_i64().unwrap();
            let e = totals.entry(pid).or_insert_with(|| json!({"id": pid, "matches": 0, "pts": 0, "k": 0, "e": 0, "n": 0, "ace": 0, "serr": 0, "sn": 0, "rsum": 0, "rn": 0, "bpts": 0, "digs": 0, "ast": 0}));
            add(e, "matches", 1);
            add(e, "pts", p["pts"].as_i64().unwrap_or(0));
            add(e, "k", p["A"]["k"].as_i64().unwrap_or(0));
            add(e, "e", p["A"]["e"].as_i64().unwrap_or(0) + p["A"]["blk"].as_i64().unwrap_or(0));
            add(e, "n", p["A"]["n"].as_i64().unwrap_or(0));
            add(e, "ace", p["S"]["ace"].as_i64().unwrap_or(0));
            add(e, "serr", p["S"]["err"].as_i64().unwrap_or(0));
            add(e, "sn", p["S"]["n"].as_i64().unwrap_or(0));
            add(e, "rsum", p["R"]["sum"].as_i64().unwrap_or(0));
            add(e, "rn", p["R"]["n"].as_i64().unwrap_or(0));
            add(e, "bpts", p["B"]["pts"].as_i64().unwrap_or(0));
            add(e, "digs", p["D"]["good"].as_i64().unwrap_or(0));
            add(e, "ast", p["E"]["ast"].as_i64().unwrap_or(0));
            a.0 += p["A"]["k"].as_i64().unwrap_or(0);
            a.1 += p["A"]["e"].as_i64().unwrap_or(0) + p["A"]["blk"].as_i64().unwrap_or(0);
            a.2 += p["A"]["n"].as_i64().unwrap_or(0);
            rr.0 += p["R"]["sum"].as_i64().unwrap_or(0);
            rr.1 += p["R"]["n"].as_i64().unwrap_or(0);
        }
        let finished = st["finished"].as_bool().unwrap_or(false);
        let sw = st["sets_won"].as_i64().unwrap_or(0);
        let sl = st["sets_lost"].as_i64().unwrap_or(0);
        if finished { add(&mut team, if sw > sl { "won" } else { "lost" }, 1); }
        add(&mut team, "sets_won", sw); add(&mut team, "sets_lost", sl);
        add(&mut team, "so_won", t["sideout"]["won"].as_i64().unwrap_or(0)); add(&mut team, "so_n", t["sideout"]["n"].as_i64().unwrap_or(0));
        add(&mut team, "brk_won", t["brk"]["won"].as_i64().unwrap_or(0)); add(&mut team, "brk_n", t["brk"]["n"].as_i64().unwrap_or(0));
        add(&mut team, "a_k", a.0); add(&mut team, "a_e", a.1); add(&mut team, "a_n", a.2);
        add(&mut team, "r_sum", rr.0); add(&mut team, "r_n", rr.1);
        let mut m = match_json(r);
        m["state"] = st.clone();
        m["sideout"] = t["sideout"].clone();
        m["brk"] = t["brk"].clone();
        m["attack"] = json!({"k": a.0, "e": a.1, "n": a.2});
        m["reception"] = json!({"sum": rr.0, "n": rr.1});
        matches.push(m);
    }
    Ok(Json(json!({
        "matches": matches,
        "players": totals.values().cloned().collect::<Vec<_>>(),
        "team": team,
    })))
}

/// the roster a new recording of this match would start with (for tests and
/// the planning view); kept here so the planning snapshot has one caller path
#[allow(dead_code)]
pub async fn planning_roster(state: &AppState, row: &sqlx::sqlite::SqliteRow) -> ApiResult<Value> {
    let mut conn = state.db.acquire().await?;
    let snap = planning_snapshot_conn(&mut conn, row).await?;
    Ok(json!(snap.roster))
}

#[allow(dead_code)]
pub async fn audit_note(state: &AppState, user: &CurrentUser, id: i64, note: &str) -> ApiResult<()> {
    audit(state, user.team_id, "match", id, "note", note, Some(user.id)).await
}
