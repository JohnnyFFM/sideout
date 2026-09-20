//! Scouting handover: one active writer (session) per match.
//!
//! Ownership lives on the match row (`scout_*` columns, migration 0007):
//! the holding session, acquisition time, last accepted write, the lease id
//! of the current ownership period, the acquisition request that produced
//! it, and a revision that advances on every transition. Every protected
//! write checks the lease inside the same write transaction that mutates
//! the log, so "check, then write" can never interleave with a takeover:
//! the write pool has one connection and the transaction holds it.
//!
//! A holder is *current* while its session and membership are valid and its
//! last accepted write is younger than ten minutes; after that it is stale
//! and a conditional `claim` may replace it. `takeover` replaces a current
//! holder, but only from the revision the user looked at.

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{Row, SqliteConnection};

use crate::auth::{self, CurrentUser, Role};
use crate::engine;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::store::{audit_conn, fetch_match_row_conn, load_actions_conn, load_config_conn};

/// minutes without an accepted write after which a holder is stale (the SQL
/// predicate below spells the same number)
pub const STALE_MINUTES: i64 = 10;
const _: () = assert!(STALE_MINUTES == 10);

/// the request header protected writes carry
pub const LEASE_HEADER: &str = "x-scout-lease";

/// SQL: the match's holder is current (a lease, a live session with scouting
/// rights in this team, and recent activity). Usable in UPDATE ... WHERE and
/// in SELECTs on `matches` (unaliased).
pub const CURRENT_HOLDER: &str = "(matches.scout_lease IS NOT NULL AND matches.scout_session_id IS NOT NULL
      AND matches.scout_seen IS NOT NULL AND matches.scout_seen > datetime('now', '-' || 10 || ' minutes')
      AND EXISTS (SELECT 1 FROM sessions s JOIN memberships mb ON mb.user_id = s.user_id AND mb.team_id = matches.team_id
                  WHERE s.id = matches.scout_session_id AND mb.role IN ('coach', 'assistant')))";

pub fn lease_of(headers: &HeaderMap) -> Option<String> {
    headers.get(LEASE_HEADER).and_then(|v| v.to_str().ok()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn iso(ts: Option<String>) -> Value {
    match ts {
        Some(t) => Value::String(format!("{}Z", t.replacen(' ', "T", 1))),
        None => Value::Null,
    }
}

/// Caller-relative scout state of a match. Never exposes the session id;
/// the lease only to the session that holds it.
pub async fn scout_json(conn: &mut SqliteConnection, match_id: i64, user: &CurrentUser) -> ApiResult<Value> {
    let r = sqlx::query(&format!(
        "SELECT matches.scout_lease, matches.scout_session_id, matches.scout_since, matches.scout_seen, matches.scout_rev,
                {CURRENT_HOLDER} AS current,
                (s.id IS NOT NULL AND EXISTS (SELECT 1 FROM memberships mb WHERE mb.user_id = s.user_id AND mb.team_id = matches.team_id
                                              AND mb.role IN ('coach', 'assistant'))) AS valid,
                u.display_name AS actor, s.device_label AS device
         FROM matches LEFT JOIN sessions s ON s.id = matches.scout_session_id LEFT JOIN users u ON u.id = s.user_id
         WHERE matches.id = ?"
    ))
    .bind(match_id)
    .fetch_one(conn)
    .await?;
    let lease: Option<String> = r.get("scout_lease");
    let sid: Option<i64> = r.get("scout_session_id");
    let valid = r.get::<i64, _>("valid") != 0 && lease.is_some();
    let current = r.get::<i64, _>("current") != 0;
    let mine = valid && sid == Some(user.session_id);
    Ok(json!({
        "mine": mine,
        "held": valid,
        "stale": valid && !current,
        "actor": if valid { r.get::<Option<String>, _>("actor").map(Value::String).unwrap_or(Value::Null) } else { Value::Null },
        "device": if valid { r.get::<Option<String>, _>("device").map(Value::String).unwrap_or(Value::Null) } else { Value::Null },
        "since": if valid { iso(r.get("scout_since")) } else { Value::Null },
        "last_seen_at": if valid { iso(r.get("scout_seen")) } else { Value::Null },
        "revision": r.get::<i64, _>("scout_rev"),
        "lease": if mine { lease.map(Value::String).unwrap_or(Value::Null) } else { Value::Null },
    }))
}

/// `current` envelope for a scout refusal: what the client needs to settle
/// its queue and show the holder
async fn current_json(conn: &mut SqliteConnection, row: &sqlx::sqlite::SqliteRow, user: &CurrentUser) -> ApiResult<Value> {
    let id: i64 = row.get("id");
    let cfg = load_config_conn(conn, id, &row.get::<String, _>("first_serve")).await?;
    let actions = load_actions_conn(conn, id).await?;
    let st = engine::replay(&cfg, &actions);
    Ok(json!({ "last_seq": st.last_seq, "state": serde_json::to_value(&st).unwrap_or(Value::Null), "scout": scout_json(conn, id, user).await? }))
}

/// A protected write: the lease from the request must be the match's
/// current lease and belong to this session. Refusals carry the current
/// state; the transaction the caller opened is rolled back by dropping it.
pub async fn require_lease(conn: &mut SqliteConnection, row: &sqlx::sqlite::SqliteRow, user: &CurrentUser, lease: Option<&str>) -> ApiResult<()> {
    let id: i64 = row.get("id");
    let r = sqlx::query(&format!("SELECT matches.scout_lease, matches.scout_session_id, {CURRENT_HOLDER} AS current FROM matches WHERE matches.id = ?"))
        .bind(id)
        .fetch_one(&mut *conn)
        .await?;
    let cur_lease: Option<String> = r.get("scout_lease");
    let cur_sid: Option<i64> = r.get("scout_session_id");
    let current = r.get::<i64, _>("current") != 0;
    let ok = lease.is_some() && cur_lease.as_deref() == lease && cur_sid == Some(user.session_id);
    if ok {
        return Ok(());
    }
    let code = if current && cur_sid != Some(user.session_id) { "scouted_elsewhere" } else { "scout_lease_expired" };
    Err(ApiError::ScoutConflict(code, current_json(conn, row, user).await?))
}

/// an accepted write keeps the holder current
pub async fn renew(conn: &mut SqliteConnection, match_id: i64) -> ApiResult<()> {
    sqlx::query("UPDATE matches SET scout_seen = datetime('now') WHERE id = ?").bind(match_id).execute(conn).await?;
    Ok(())
}

#[derive(Deserialize)]
pub struct AcquireBody {
    pub mode: String,
    /// the ownership revision the client looked at
    pub revision: i64,
    /// stable id of this acquisition attempt, so a retry after a lost answer
    /// is answered with the same lease instead of a second period
    pub request_id: String,
}

/// POST /matches/{id}/scout — `claim` (only when unheld or stale) or
/// `takeover` (replaces the holder the user saw). The conditional UPDATE
/// decides: whoever's statement runs first wins, the other sees 0 rows.
pub async fn acquire(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(body): Json<AcquireBody>,
) -> ApiResult<Json<Value>> {
    user.require(Role::Assistant)?;
    if body.request_id.trim().is_empty() || body.request_id.len() > 80 {
        return Err(ApiError::BadRequest("request_id fehlt".into()));
    }
    if !matches!(body.mode.as_str(), "claim" | "takeover") {
        return Err(ApiError::BadRequest("mode muss claim oder takeover sein".into()));
    }
    let mut tx = state.dbw.begin().await?;
    let row = fetch_match_row_conn(&mut tx, user.team_id, id).await?;
    let cur_lease: Option<String> = row.get("scout_lease");
    let cur_sid: Option<i64> = row.get("scout_session_id");
    let cur_req: Option<String> = row.get("scout_req");
    let cur_rev: i64 = row.get("scout_rev");

    // a retry of the acquisition that produced the current lease
    if cur_lease.is_some() && cur_sid == Some(user.session_id) && cur_req.as_deref() == Some(body.request_id.as_str()) {
        tx.commit().await?;
        return Ok(Json(super::matches::match_payload(&state, &user, id).await?));
    }
    let lease = auth::new_token();
    let res = if body.mode == "claim" {
        // free or stale, or held by this very session (another tab of it
        // handing over): a new period, so the old tab's late release of its
        // lease is a no-op instead of clearing this one
        sqlx::query(&format!(
            "UPDATE matches SET scout_session_id = ?, scout_since = datetime('now'), scout_seen = datetime('now'),
                    scout_lease = ?, scout_req = ?, scout_rev = scout_rev + 1
             WHERE matches.id = ? AND matches.scout_rev = ? AND (NOT {CURRENT_HOLDER} OR matches.scout_session_id = ?)"
        ))
        .bind(user.session_id).bind(&lease).bind(&body.request_id).bind(id).bind(body.revision).bind(user.session_id)
        .execute(&mut *tx)
        .await?
    } else {
        sqlx::query(
            "UPDATE matches SET scout_session_id = ?, scout_since = datetime('now'), scout_seen = datetime('now'),
                    scout_lease = ?, scout_req = ?, scout_rev = scout_rev + 1
             WHERE matches.id = ? AND matches.scout_rev = ?",
        )
        .bind(user.session_id).bind(&lease).bind(&body.request_id).bind(id).bind(body.revision)
        .execute(&mut *tx)
        .await?
    };
    if res.rows_affected() == 0 {
        // someone else holds it, or ownership moved since the client looked
        let cur = current_json(&mut tx, &row, &user).await?;
        let code = if cur["scout"]["held"] == true && cur["scout"]["stale"] == false && cur["scout"]["mine"] == false { "scouted_elsewhere" } else { "scout_lease_expired" };
        return Err(ApiError::ScoutConflict(code, cur));
    }
    let _ = (cur_rev, cur_lease, cur_sid);
    audit_conn(&mut tx, user.team_id, "match", id, if body.mode == "claim" { "scout_claim" } else { "scout_takeover" }, &user.display_name, Some(user.id)).await?;
    tx.commit().await?;
    let rev: i64 = sqlx::query("SELECT scout_rev FROM matches WHERE id = ?").bind(id).fetch_one(&state.db).await?.get("scout_rev");
    super::matches::publish(&state, &user, "match", id, rev, "scout");
    Ok(Json(super::matches::match_payload(&state, &user, id).await?))
}

#[derive(Deserialize)]
pub struct ReleaseQuery {
    pub lease: Option<String>,
}

/// DELETE /matches/{id}/scout?lease=… — release only when this session
/// holds exactly that lease; anything else is a 204 no-op (a late release
/// from an old page must not clear a newer period).
pub async fn release(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Query(q): Query<ReleaseQuery>,
) -> ApiResult<impl IntoResponse> {
    let Some(lease) = q.lease.filter(|l| !l.is_empty()) else { return Ok(StatusCode::NO_CONTENT) };
    let mut tx = state.dbw.begin().await?;
    fetch_match_row_conn(&mut tx, user.team_id, id).await?;
    let res = sqlx::query(
        "UPDATE matches SET scout_session_id = NULL, scout_since = NULL, scout_seen = NULL, scout_lease = NULL, scout_req = NULL,
                scout_rev = scout_rev + 1
         WHERE id = ? AND scout_session_id = ? AND scout_lease = ?",
    )
    .bind(id).bind(user.session_id).bind(&lease)
    .execute(&mut *tx)
    .await?;
    if res.rows_affected() == 0 {
        return Ok(StatusCode::NO_CONTENT);
    }
    audit_conn(&mut tx, user.team_id, "match", id, "scout_release", &user.display_name, Some(user.id)).await?;
    tx.commit().await?;
    let rev: i64 = sqlx::query("SELECT scout_rev FROM matches WHERE id = ?").bind(id).fetch_one(&state.db).await?.get("scout_rev");
    super::matches::publish(&state, &user, "match", id, rev, "scout");
    Ok(StatusCode::NO_CONTENT)
}
