//! Matches, lineups, the scout log (actions), replayed state, stats, export,
//! season aggregates.

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
use crate::state::AppState;
use crate::store::{
    action_json, audit, fetch_match_row, lineups_json, load_actions, load_config, load_players, match_json,
};

fn publish(state: &AppState, user: &CurrentUser, entity: &str, id: i64, version: i64, action: &str) {
    state.events.publish(EventMsg {
        team_id: user.team_id,
        entity: entity.into(),
        id,
        version,
        action: action.into(),
        actor: user.display_name.clone(),
    });
}

async fn state_json(state: &AppState, row: &sqlx::sqlite::SqliteRow) -> ApiResult<Value> {
    let id: i64 = row.get("id");
    let cfg = load_config(state, id, &row.get::<String, _>("first_serve")).await?;
    let actions = load_actions(state, id).await?;
    Ok(serde_json::to_value(engine::replay(&cfg, &actions)).unwrap_or(Value::Null))
}

pub async fn list(State(state): State<AppState>, user: CurrentUser) -> ApiResult<Json<Value>> {
    let rows = sqlx::query("SELECT * FROM matches WHERE team_id = ? ORDER BY date DESC, id DESC")
        .bind(user.team_id)
        .fetch_all(&state.db)
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        let mut m = match_json(r);
        m["state"] = state_json(&state, r).await?;
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

async fn write_lineup(state: &AppState, match_id: i64, set: i64, l: &LineupBody) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO lineups (match_id, set_no, pos1, pos2, pos3, pos4, pos5, pos6, libero_id)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(match_id, set_no) DO UPDATE SET pos1=excluded.pos1, pos2=excluded.pos2, pos3=excluded.pos3,
           pos4=excluded.pos4, pos5=excluded.pos5, pos6=excluded.pos6, libero_id=excluded.libero_id",
    )
    .bind(match_id).bind(set)
    .bind(l.pos[0]).bind(l.pos[1]).bind(l.pos[2]).bind(l.pos[3]).bind(l.pos[4]).bind(l.pos[5])
    .bind(l.libero)
    .execute(&state.dbw)
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
    .execute(&state.dbw)
    .await?;
    let id = res.last_insert_rowid();
    if let Some(l) = &body.lineup {
        write_lineup(&state, id, 1, l).await?;
    }
    audit(&state, user.team_id, "match", id, "created", body.opponent.trim(), Some(user.id)).await?;
    publish(&state, &user, "match", id, 1, "created");
    get_one(State(state), user, Path(id)).await
}

pub async fn get_one(State(state): State<AppState>, user: CurrentUser, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let row = fetch_match_row(&state, user.team_id, id).await?;
    let cfg = load_config(&state, id, &row.get::<String, _>("first_serve")).await?;
    let actions = sqlx::query("SELECT * FROM actions WHERE match_id = ? ORDER BY seq")
        .bind(id)
        .fetch_all(&state.db)
        .await?;
    let players = sqlx::query("SELECT * FROM players WHERE team_id = ? ORDER BY number")
        .bind(user.team_id)
        .fetch_all(&state.db)
        .await?;
    let mut m = match_json(&row);
    m["lineups"] = lineups_json(&cfg);
    m["actions"] = json!(actions.iter().map(action_json).collect::<Vec<_>>());
    m["players"] = json!(players.iter().map(crate::store::player_json).collect::<Vec<_>>());
    m["state"] = state_json(&state, &row).await?;
    Ok(Json(m))
}

#[derive(Deserialize)]
pub struct PatchMatch {
    pub version: i64,
    pub opponent: Option<String>,
    pub date: Option<String>,
    pub time: Option<String>,
    pub hall: Option<String>,
    pub home: Option<bool>,
    pub first_serve: Option<String>,
    pub status: Option<String>,
    pub notes: Option<String>,
}

pub async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(body): Json<PatchMatch>,
) -> ApiResult<Json<Value>> {
    user.require(Role::Assistant)?;
    let cur = fetch_match_row(&state, user.team_id, id).await?;
    let opponent = body.opponent.clone().unwrap_or(cur.get("opponent"));
    let date = body.date.clone().unwrap_or(cur.get("date"));
    let first_serve = body.first_serve.clone().unwrap_or(cur.get("first_serve"));
    validate_match(&opponent, &date, &first_serve)?;
    let status = body.status.clone().unwrap_or(cur.get("status"));
    if !matches!(status.as_str(), "planned" | "live" | "done") {
        return Err(ApiError::BadRequest("Status unbekannt".into()));
    }
    let res = sqlx::query(
        "UPDATE matches SET opponent = ?, date = ?, time = ?, hall = ?, home = ?, first_serve = ?, status = ?, notes = ?,
         version = version + 1, updated_at = datetime('now') WHERE id = ? AND version = ?",
    )
    .bind(opponent.trim())
    .bind(&date)
    .bind(body.time.clone().or(cur.get("time")).filter(|t| !t.is_empty()))
    .bind(body.hall.clone().unwrap_or(cur.get("hall")).trim())
    .bind(body.home.map(|b| b as i64).unwrap_or(cur.get("home")))
    .bind(&first_serve)
    .bind(&status)
    .bind(body.notes.clone().unwrap_or(cur.get("notes")).trim())
    .bind(id)
    .bind(body.version)
    .execute(&state.dbw)
    .await?;
    if res.rows_affected() == 0 {
        let current = fetch_match_row(&state, user.team_id, id).await?;
        return Err(ApiError::Conflict(match_json(&current)));
    }
    audit(&state, user.team_id, "match", id, "updated", &status, Some(user.id)).await?;
    publish(&state, &user, "match", id, body.version + 1, "updated");
    get_one(State(state), user, Path(id)).await
}

pub async fn remove(State(state): State<AppState>, user: CurrentUser, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    user.require(Role::Coach)?;
    fetch_match_row(&state, user.team_id, id).await?;
    sqlx::query("DELETE FROM matches WHERE id = ?").bind(id).execute(&state.dbw).await?;
    audit(&state, user.team_id, "match", id, "deleted", "", Some(user.id)).await?;
    publish(&state, &user, "match", id, 0, "deleted");
    Ok(Json(json!({ "ok": true })))
}

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
    let row = fetch_match_row(&state, user.team_id, id).await?;
    validate_lineup(&state, user.team_id, &body).await?;
    write_lineup(&state, id, set, &body).await?;
    audit(&state, user.team_id, "match", id, "lineup", &format!("Satz {set}"), Some(user.id)).await?;
    publish(&state, &user, "match", id, row.get("version"), "lineup");
    get_one(State(state), user, Path(id)).await
}

#[derive(Deserialize)]
pub struct NewAction {
    pub seq: i64,
    pub skill: String,
    pub grade: Option<String>,
    pub player_id: Option<i64>,
    pub sub_out: Option<i64>,
    pub sub_in: Option<i64>,
}

/// Append one action. `seq` must be last_seq + 1; a retry with the same seq
/// and payload is answered with the stored row (idempotent), a different
/// payload on a taken seq is a 409 with the current state.
pub async fn add_action(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(body): Json<NewAction>,
) -> ApiResult<Json<Value>> {
    user.require(Role::Assistant)?;
    let row = fetch_match_row(&state, user.team_id, id).await?;
    match body.skill.as_str() {
        "S" | "R" | "E" | "A" | "B" | "D" => {
            let g = body.grade.as_deref().unwrap_or("");
            if !engine::GRADES.contains(&g) {
                return Err(ApiError::BadRequest("Bewertung fehlt".into()));
            }
            if body.player_id.is_none() {
                return Err(ApiError::BadRequest("Spielerin fehlt".into()));
            }
        }
        "opp" => {
            if !matches!(body.grade.as_deref(), Some("#") | Some("=")) {
                return Err(ApiError::BadRequest("Gegner-Aktion braucht # oder =".into()));
            }
        }
        "sub" => {
            if body.sub_out.is_none() || body.sub_in.is_none() {
                return Err(ApiError::BadRequest("Wechsel braucht raus und rein".into()));
            }
        }
        _ => return Err(ApiError::BadRequest("Aktion unbekannt".into())),
    }
    let cfg = load_config(&state, id, &row.get::<String, _>("first_serve")).await?;
    if cfg.lineups.is_empty() {
        return Err(ApiError::BadRequest("Erst die Aufstellung eintragen".into()));
    }
    let actions = load_actions(&state, id).await?;
    let st = engine::replay(&cfg, &actions);

    // idempotent retry?
    if let Some(existing) = actions.iter().find(|a| a.seq == body.seq) {
        let same = existing.skill == body.skill
            && existing.grade == body.grade
            && existing.player_id == body.player_id
            && existing.sub_out == body.sub_out
            && existing.sub_in == body.sub_in;
        if same {
            let r = sqlx::query("SELECT * FROM actions WHERE id = ?").bind(existing.id).fetch_one(&state.db).await?;
            return Ok(Json(json!({ "action": action_json(&r), "state": serde_json::to_value(&st).unwrap_or(Value::Null), "duplicate": true })));
        }
        return Err(ApiError::Conflict(json!({ "last_seq": st.last_seq, "state": serde_json::to_value(&st).unwrap_or(Value::Null) })));
    }
    if body.seq != st.last_seq + 1 {
        return Err(ApiError::Conflict(json!({ "last_seq": st.last_seq, "state": serde_json::to_value(&st).unwrap_or(Value::Null) })));
    }
    if st.finished {
        return Err(ApiError::BadRequest("Das Spiel ist beendet".into()));
    }
    let res = sqlx::query(
        "INSERT INTO actions (match_id, seq, set_no, skill, grade, player_id, sub_out, sub_in, created_by)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id).bind(body.seq).bind(st.set)
    .bind(&body.skill)
    .bind(if body.skill == "sub" { None } else { body.grade.clone() })
    .bind(if body.skill == "sub" || body.skill == "opp" { None } else { body.player_id })
    .bind(body.sub_out).bind(body.sub_in)
    .bind(user.id)
    .execute(&state.dbw)
    .await?;
    let action_id = res.last_insert_rowid();
    // first action flips a planned match to live; a finishing action to done
    let mut actions2 = actions;
    actions2.push(engine::Action {
        id: action_id, seq: body.seq, skill: body.skill.clone(),
        grade: if body.skill == "sub" { None } else { body.grade.clone() },
        player_id: body.player_id, sub_out: body.sub_out, sub_in: body.sub_in,
    });
    let st2 = engine::replay(&cfg, &actions2);
    let status: String = row.get("status");
    let new_status = if st2.finished { "done" } else if status == "planned" { "live" } else { status.as_str() };
    if new_status != status {
        sqlx::query("UPDATE matches SET status = ?, version = version + 1, updated_at = datetime('now') WHERE id = ?")
            .bind(new_status).bind(id).execute(&state.dbw).await?;
        publish(&state, &user, "match", id, 0, "status");
    }
    publish(&state, &user, "action", id, body.seq, "added");
    let r = sqlx::query("SELECT * FROM actions WHERE id = ?").bind(action_id).fetch_one(&state.db).await?;
    Ok(Json(json!({ "action": action_json(&r), "state": serde_json::to_value(&st2).unwrap_or(Value::Null) })))
}

pub async fn undo_action(State(state): State<AppState>, user: CurrentUser, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    user.require(Role::Assistant)?;
    let row = fetch_match_row(&state, user.team_id, id).await?;
    let last = sqlx::query("SELECT * FROM actions WHERE match_id = ? ORDER BY seq DESC LIMIT 1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    let Some(last) = last else {
        return Err(ApiError::BadRequest("Nichts zum Rückgängigmachen".into()));
    };
    let last_id: i64 = last.get("id");
    let seq: i64 = last.get("seq");
    sqlx::query("DELETE FROM actions WHERE id = ?").bind(last_id).execute(&state.dbw).await?;
    audit(&state, user.team_id, "match", id, "undo", &format!("seq {seq}"), Some(user.id)).await?;
    // a done match that is reopened by undo goes back to live
    if row.get::<String, _>("status") == "done" {
        sqlx::query("UPDATE matches SET status = 'live', version = version + 1, updated_at = datetime('now') WHERE id = ?")
            .bind(id).execute(&state.dbw).await?;
        publish(&state, &user, "match", id, 0, "status");
    }
    publish(&state, &user, "action", id, seq - 1, "undone");
    let cfg = load_config(&state, id, &row.get::<String, _>("first_serve")).await?;
    let actions = load_actions(&state, id).await?;
    Ok(Json(json!({ "removed": action_json(&last), "state": serde_json::to_value(engine::replay(&cfg, &actions)).unwrap_or(Value::Null) })))
}

pub async fn state(State(state): State<AppState>, user: CurrentUser, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let row = fetch_match_row(&state, user.team_id, id).await?;
    Ok(Json(state_json(&state, &row).await?))
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
    let cfg = load_config(&state, id, &row.get::<String, _>("first_serve")).await?;
    let actions = load_actions(&state, id).await?;
    let players = load_players(&state, user.team_id).await?;
    Ok(Json(engine::stats(&cfg, &players, &actions, q.set.filter(|s| *s > 0))))
}

pub async fn export_csv(State(state): State<AppState>, user: CurrentUser, Path(id): Path<i64>) -> ApiResult<impl IntoResponse> {
    let row = fetch_match_row(&state, user.team_id, id).await?;
    let cfg = load_config(&state, id, &row.get::<String, _>("first_serve")).await?;
    let actions = load_actions(&state, id).await?;
    let players = load_players(&state, user.team_id).await?;
    let csv = engine::export_csv(&cfg, &players, &actions);
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
/// all of them. Everything derives from the logs, nothing is stored twice.
pub async fn season(State(state): State<AppState>, user: CurrentUser) -> ApiResult<Json<Value>> {
    let rows = sqlx::query("SELECT * FROM matches WHERE team_id = ? AND status IN ('live','done') ORDER BY date, id")
        .bind(user.team_id)
        .fetch_all(&state.db)
        .await?;
    let players = load_players(&state, user.team_id).await?;
    let mut matches = vec![];
    let mut totals: std::collections::BTreeMap<i64, Value> = Default::default();
    let mut team = json!({"so_won": 0, "so_n": 0, "brk_won": 0, "brk_n": 0, "a_k": 0, "a_e": 0, "a_n": 0, "r_sum": 0, "r_n": 0, "won": 0, "lost": 0, "sets_won": 0, "sets_lost": 0});
    let add = |v: &mut Value, k: &str, d: i64| { v[k] = json!(v[k].as_i64().unwrap_or(0) + d); };
    for r in &rows {
        let id: i64 = r.get("id");
        let cfg = load_config(&state, id, &r.get::<String, _>("first_serve")).await?;
        let actions = load_actions(&state, id).await?;
        let s = engine::stats(&cfg, &players, &actions, None);
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
