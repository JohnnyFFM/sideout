//! Recording upload, download and the coach's selection.
//!
//! Upload never fails for ownership reasons: any assistant or coach of the
//! match's team may store a recording, whatever team the session currently
//! shows. The team is resolved from the match, never taken from the client.

use axum::extract::{Path, State};
use axum::Json;
use serde_json::{json, Value};
use sqlx::Row;

use crate::auth::{CurrentUser, Role};
use crate::engine;
use crate::error::{ApiError, ApiResult};
use crate::events::EventMsg;
use crate::recording::{self, base_from_snapshot, parse_base, parse_edit, Base};
use crate::state::AppState;
use crate::store::{
    audit_conn, fetch_match_any_conn, load_recording_conn, rec_meta_json, recording_meta_conn, refresh_status_conn,
    snapshot_json, state_summary, team_role_conn,
};

/// the match's team and the caller's role in it (session team irrelevant)
async fn match_and_role(conn: &mut sqlx::SqliteConnection, user: &CurrentUser, id: i64, min: Role) -> ApiResult<(sqlx::sqlite::SqliteRow, i64)> {
    let row = fetch_match_any_conn(conn, id).await?;
    let team_id: i64 = row.get("team_id");
    match team_role_conn(conn, user.id, team_id).await? {
        Some(r) if r >= min => Ok((row, team_id)),
        Some(_) => Err(ApiError::Forbidden),
        None => Err(ApiError::NotFound),
    }
}

fn publish(state: &AppState, team_id: i64, user: &CurrentUser, id: i64, version: i64, action: &str) {
    state.events.publish(EventMsg {
        team_id,
        entity: "match".into(),
        id,
        version,
        action: action.into(),
        actor: user.display_name.clone(),
    });
}

fn valid_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn str_field(v: &Value, k: &str, max: usize) -> String {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("").trim().chars().take(max).collect()
}

/// PUT /matches/{id}/recordings/{rid}
/// `{ uploader?, device_id, device_label, origin_id?, origin_n?, base?, edits: [{ n, body }] }`
/// The base is required on the first upload and must be identical on any
/// later one. Edits are stored while their numbers continue the stored
/// ones; the answer names the highest stored number, the client resends from
/// there. `uploader` is the account the client believes it is signed in as:
/// a session cookie that meanwhile belongs to someone else (another tab
/// switched accounts) is refused, so a recording never lands under the wrong
/// account. Everything in one write transaction.
pub async fn upload(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((id, rid)): Path<(i64, String)>,
    Json(body): Json<Value>,
) -> ApiResult<Json<Value>> {
    if !valid_id(&rid) {
        return Err(ApiError::BadRequest("Aufzeichnungs-ID ungültig".into()));
    }
    if let Some(expected) = body.get("uploader").and_then(|u| u.as_i64()) {
        if expected != user.id {
            return Err(ApiError::Refused("account_mismatch", json!({ "user_id": user.id, "username": user.username })));
        }
    }
    let mut tx = state.dbw.begin().await?;
    let (row, team_id) = match_and_role(&mut tx, &user, id, Role::Assistant).await?;
    let device_id = str_field(&body, "device_id", 64);
    let device_label = str_field(&body, "device_label", 40);
    let existing = sqlx::query("SELECT match_id, base, device_id FROM recordings WHERE id = ?").bind(&rid).fetch_optional(&mut *tx).await?;
    let base_in = body.get("base").filter(|b| !b.is_null());
    // a new copy of a recording: where it was taken from, how far that was, and its base
    let mut new_copy: Option<(String, i64, Base)> = None;
    match &existing {
        Some(r) => {
            if r.get::<i64, _>("match_id") != id {
                return Err(ApiError::Refused("recording_mismatch", json!({ "reason": "Aufzeichnung gehört zu einem anderen Spiel" })));
            }
            if let Some(b) = base_in {
                let (_, canon) = parse_base(b).map_err(ApiError::BadRequest)?;
                if canon != r.get::<String, _>("base") {
                    return Err(ApiError::Refused("recording_mismatch", json!({ "reason": "Anfangsstand weicht vom gespeicherten ab" })));
                }
            }
            // the match's migrated recording (no device yet) becomes the recording of
            // the first device that continues it: from then on it is that device's own
            if r.get::<String, _>("device_id") == "legacy" && !device_id.is_empty() && device_id != "legacy" {
                sqlx::query("UPDATE recordings SET device_id = ?, imported = 0 WHERE id = ?").bind(&device_id).bind(&rid).execute(&mut *tx).await?;
                audit_conn(&mut tx, team_id, "match", id, "recording_adopted", &rid, Some(user.id)).await?;
            }
        }
        None => {
            let Some(b) = base_in else { return Err(ApiError::BadRequest("Anfangsstand fehlt".into())) };
            let (parsed, canon) = parse_base(b).map_err(ApiError::BadRequest)?;
            let origin_id = body.get("origin_id").and_then(|x| x.as_str()).filter(|s| valid_id(s)).map(str::to_string);
            let origin_n = body.get("origin_n").and_then(|x| x.as_i64());
            let imported = body.get("imported").and_then(|x| x.as_bool()).unwrap_or(false);
            sqlx::query(
                "INSERT INTO recordings (id, match_id, team_id, user_id, device_id, device_label, origin_id, origin_n, base, imported)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&rid).bind(id).bind(team_id).bind(user.id).bind(&device_id).bind(&device_label).bind(origin_id.as_deref()).bind(origin_n).bind(&canon).bind(imported as i64)
            .execute(&mut *tx)
            .await?;
            audit_conn(&mut tx, team_id, "match", id, "recording_new", &rid, Some(user.id)).await?;
            if !imported {
                if let (Some(o), Some(n)) = (origin_id, origin_n) {
                    new_copy = Some((o, n, parsed));
                }
            }
        }
    }
    let mut confirmed: i64 = sqlx::query("SELECT coalesce(max(n), 0) AS n FROM edits WHERE recording_id = ?")
        .bind(&rid)
        .fetch_one(&mut *tx)
        .await?
        .get("n");
    let mut edits: Vec<(i64, &Value)> = body
        .get("edits")
        .and_then(|e| e.as_array())
        .map(|a| a.iter().filter_map(|e| Some((e.get("n")?.as_i64()?, e.get("body")?))).collect())
        .unwrap_or_default();
    edits.sort_by_key(|(n, _)| *n);
    let mut stored = 0;
    for (n, v) in edits {
        if n < 1 {
            return Err(ApiError::BadRequest("Editnummer ungültig".into()));
        }
        let (_, canon) = parse_edit(v).map_err(|e| ApiError::BadRequest(format!("Edit {n}: {e}")))?;
        if n <= confirmed {
            let have: String = sqlx::query("SELECT body FROM edits WHERE recording_id = ? AND n = ?").bind(&rid).bind(n).fetch_one(&mut *tx).await?.get("body");
            if have != canon {
                return Err(ApiError::Refused("edit_mismatch", json!({ "n": n, "confirmed": confirmed })));
            }
            continue;
        }
        if n != confirmed + 1 {
            break; // a gap: the answer says where we are, the client resends from there
        }
        sqlx::query("INSERT INTO edits (recording_id, n, body) VALUES (?, ?, ?)").bind(&rid).bind(n).bind(&canon).execute(&mut *tx).await?;
        confirmed = n;
        stored += 1;
    }
    if stored > 0 || existing.is_none() {
        sqlx::query("UPDATE recordings SET last_write = datetime('now'), device_label = CASE WHEN ? = '' THEN device_label ELSE ? END WHERE id = ?")
            .bind(&device_label).bind(&device_label).bind(&rid)
            .execute(&mut *tx)
            .await?;
    }
    // the only recording of a match is its result
    let selected_before: Option<String> = row.get("selected_recording");
    if selected_before.is_none() {
        sqlx::query("UPDATE matches SET selected_recording = ?, selection_rev = selection_rev + 1, updated_at = datetime('now') WHERE id = ? AND selected_recording IS NULL")
            .bind(&rid).bind(id).execute(&mut *tx).await?;
    }
    // continuing the result on another device: a copy taken from the result
    // while it had exactly that many edits — nothing came in between — is the
    // result from now on; the coach only picks when two recordings differ.
    // The claim is checked against the stored data: the copy's base must be
    // the result's folded state (its roster may add today's players).
    let mut promoted = false;
    if let Some((oid, on, got)) = &new_copy {
        if selected_before.as_deref() == Some(oid.as_str()) {
            if continues_exactly(&mut tx, oid, *on, got).await? {
                sqlx::query("UPDATE matches SET selected_recording = ?, selection_rev = selection_rev + 1, updated_at = datetime('now') WHERE id = ?")
                    .bind(&rid).bind(id).execute(&mut *tx).await?;
                audit_conn(&mut tx, team_id, "match", id, "recording_continued", &rid, Some(user.id)).await?;
                promoted = true;
            }
        }
    }
    refresh_status_conn(&mut tx, id).await?;
    if stored > 0 {
        audit_conn(&mut tx, team_id, "match", id, "recording", &format!("{rid} bis {confirmed}"), Some(user.id)).await?;
    }
    let cur = fetch_match_any_conn(&mut tx, id).await?;
    let selected: Option<String> = cur.get("selected_recording");
    let snap = crate::store::snapshot_of_conn(&mut tx, &rid).await?.ok_or(ApiError::NotFound)?;
    tx.commit().await?;
    if stored > 0 || existing.is_none() {
        publish(&state, team_id, &user, id, cur.get("version"), "recording");
    }
    if promoted {
        publish(&state, team_id, &user, id, 0, "selected");
    }
    let st = engine::replay(&snap.cfg, &snap.actions);
    Ok(Json(json!({
        "confirmed": confirmed,
        "selected": selected.as_deref() == Some(rid.as_str()),
        "status": cur.get::<String, _>("status"),
        "state": state_summary(&st),
    })))
}

/// Does a base equal the folded state of recording `oid` after exactly its
/// first `n` edits, with `n` being all the recording has? Serve, lineups and
/// actions must match; the roster may only have grown.
async fn continues_exactly(conn: &mut sqlx::SqliteConnection, oid: &str, n: i64, got: &Base) -> ApiResult<bool> {
    let Some((ob, oe)) = load_recording_conn(conn, oid).await? else { return Ok(false) };
    if n < 0 || oe.len() != n as usize {
        return Ok(false);
    }
    let want = base_from_snapshot(&recording::fold(&ob, &oe));
    Ok(want.first_serve_us == got.first_serve_us
        && want.lineups == got.lineups
        && want.actions == got.actions
        && want.roster.iter().all(|p| got.roster.iter().any(|q| q.id == p.id)))
}

/// GET /matches/{id}/recordings/{rid} — everything: meta, base, edits and
/// the folded state (what a copy or an export takes)
pub async fn get_recording(State(state): State<AppState>, user: CurrentUser, Path((id, rid)): Path<(i64, String)>) -> ApiResult<Json<Value>> {
    let mut conn = state.db.acquire().await?;
    match_and_role(&mut conn, &user, id, Role::Viewer).await?;
    let meta = recording_meta_conn(&mut conn, &rid).await?.filter(|m| m.match_id == id).ok_or(ApiError::NotFound)?;
    let (base, edits) = load_recording_conn(&mut conn, &rid).await?.ok_or(ApiError::NotFound)?;
    let snap = recording::fold(&base, &edits);
    let st = engine::replay(&snap.cfg, &snap.actions);
    let mut m = rec_meta_json(&meta);
    m["base"] = serde_json::to_value(&base).unwrap_or(Value::Null);
    m["edits"] = json!(edits.iter().enumerate().map(|(i, e)| json!({ "n": i as i64 + 1, "body": e })).collect::<Vec<_>>());
    m["snapshot"] = snapshot_json(&snap);
    m["state"] = state_summary(&st);
    Ok(Json(m))
}

/// DELETE /matches/{id}/recordings/{rid} — a coach of the team or the
/// creator hides a recording that is not the result. A flag, never a prune:
/// the rows stay, a late upload into it is still stored.
pub async fn delete_recording(State(state): State<AppState>, user: CurrentUser, Path((id, rid)): Path<(i64, String)>) -> ApiResult<Json<Value>> {
    let mut tx = state.dbw.begin().await?;
    let (row, team_id) = match_and_role(&mut tx, &user, id, Role::Assistant).await?;
    let rec = recording_meta_conn(&mut tx, &rid).await?.filter(|m| m.match_id == id).ok_or(ApiError::NotFound)?;
    let role = team_role_conn(&mut tx, user.id, team_id).await?.unwrap_or(Role::Viewer);
    if role < Role::Coach && rec.user_id != Some(user.id) {
        return Err(ApiError::Forbidden);
    }
    if row.get::<Option<String>, _>("selected_recording").as_deref() == Some(rid.as_str()) {
        return Err(ApiError::BadRequest("Das Ergebnis kann nicht gelöscht werden; erst eine andere Aufzeichnung als Ergebnis wählen".into()));
    }
    if !rec.deleted {
        sqlx::query("UPDATE recordings SET deleted = 1 WHERE id = ?").bind(&rid).execute(&mut *tx).await?;
        audit_conn(&mut tx, team_id, "match", id, "recording_deleted", &rid, Some(user.id)).await?;
    }
    tx.commit().await?;
    publish(&state, team_id, &user, id, 0, "recording");
    Ok(Json(json!({ "ok": true, "id": rid })))
}

#[derive(serde::Deserialize)]
pub struct SelectBody {
    pub recording_id: String,
    /// the selection revision the coach looked at
    pub rev: i64,
}

/// POST /matches/{id}/select — the coach picks the recording that counts
pub async fn select(State(state): State<AppState>, user: CurrentUser, Path(id): Path<i64>, Json(body): Json<SelectBody>) -> ApiResult<Json<Value>> {
    let mut tx = state.dbw.begin().await?;
    let (row, team_id) = match_and_role(&mut tx, &user, id, Role::Coach).await?;
    let rec = recording_meta_conn(&mut tx, &body.recording_id).await?.filter(|m| m.match_id == id && !m.deleted).ok_or(ApiError::NotFound)?;
    let res = sqlx::query("UPDATE matches SET selected_recording = ?, selection_rev = selection_rev + 1, updated_at = datetime('now') WHERE id = ? AND selection_rev = ?")
        .bind(&rec.id).bind(id).bind(body.rev)
        .execute(&mut *tx)
        .await?;
    if res.rows_affected() == 0 {
        let cur = fetch_match_any_conn(&mut tx, id).await?;
        return Err(ApiError::Refused("selection_moved", json!({ "selected": cur.get::<Option<String>, _>("selected_recording"), "selection_rev": cur.get::<i64, _>("selection_rev") })));
    }
    refresh_status_conn(&mut tx, id).await?;
    audit_conn(&mut tx, team_id, "match", id, "recording_selected", &rec.id, Some(user.id)).await?;
    let _ = row;
    tx.commit().await?;
    publish(&state, team_id, &user, id, 0, "selected");
    Ok(Json(super::matches::match_payload(&state, &user, id).await?))
}
