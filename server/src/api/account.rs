//! The login itself (name, password) and membership self-service (leave a
//! team, per-team detail for the Teams page).

use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;

use crate::auth::{self, CurrentUser, Role};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::store::audit;

use super::auth_routes::me_payload;

#[derive(Deserialize)]
pub struct PatchMe {
    pub display_name: Option<String>,
}

/// PATCH /me — the display name; empty means "show my login".
pub async fn patch_me(State(state): State<AppState>, user: CurrentUser, Json(body): Json<PatchMe>) -> ApiResult<Json<Value>> {
    if let Some(n) = body.display_name.as_deref() {
        let name = n.trim();
        let name = if name.is_empty() { user.username.as_str() } else { name };
        if name.chars().count() > 60 {
            return Err(ApiError::BadRequest("Anzeigename zu lang (max. 60 Zeichen)".into()));
        }
        sqlx::query("UPDATE users SET display_name = ? WHERE id = ?").bind(name).bind(user.id).execute(&state.dbw).await?;
    }
    let fresh = CurrentUser {
        display_name: sqlx::query("SELECT display_name FROM users WHERE id = ?").bind(user.id).fetch_one(&state.db).await?.get("display_name"),
        ..user
    };
    Ok(Json(me_payload(&state, &fresh).await?))
}

#[derive(Deserialize)]
pub struct ChangePassword {
    pub current: String,
    pub new: String,
}

/// POST /me/password — needs the current password; other devices stay logged in.
pub async fn change_password(State(state): State<AppState>, user: CurrentUser, Json(body): Json<ChangePassword>) -> ApiResult<Json<Value>> {
    let hash: String = sqlx::query("SELECT password_hash FROM users WHERE id = ?").bind(user.id).fetch_one(&state.db).await?.get("password_hash");
    if !auth::verify_password(&body.current, &hash) {
        return Err(ApiError::BadRequest("Das aktuelle Passwort stimmt nicht".into()));
    }
    if body.new.len() < 8 {
        return Err(ApiError::BadRequest("Passwort zu kurz (min. 8 Zeichen)".into()));
    }
    if body.new.to_lowercase() == user.username.to_lowercase() {
        return Err(ApiError::BadRequest("Das Passwort darf nicht der Benutzername sein".into()));
    }
    let new_hash = auth::hash_password(&body.new).map_err(ApiError::Internal)?;
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?").bind(&new_hash).bind(user.id).execute(&state.dbw).await?;
    Ok(Json(json!({ "ok": true })))
}

/// DELETE /teams/{id}/membership — leave a team. The last coach cannot
/// leave (the team would be orphaned); if it was this device's active
/// team, another membership becomes active, or none is left and the
/// client sends the user to the login.
pub async fn leave_team(State(state): State<AppState>, user: CurrentUser, Path(team_id): Path<i64>) -> ApiResult<Json<Value>> {
    let m = sqlx::query("SELECT role FROM memberships WHERE user_id = ? AND team_id = ?")
        .bind(user.id).bind(team_id).fetch_optional(&state.db).await?
        .ok_or(ApiError::NotFound)?;
    if m.get::<String, _>("role") == "coach" {
        let coaches: i64 = sqlx::query("SELECT count(*) AS n FROM memberships WHERE team_id = ? AND role = 'coach'")
            .bind(team_id).fetch_one(&state.db).await?.get("n");
        if coaches <= 1 {
            return Err(ApiError::BadRequest("Du bist die letzte Trainer:in dieses Teams. Ernenne erst jemand anderen zur Trainer:in.".into()));
        }
    }
    sqlx::query("DELETE FROM memberships WHERE user_id = ? AND team_id = ?").bind(user.id).bind(team_id).execute(&state.dbw).await?;
    audit(&state, team_id, "team", team_id, "member_left", &user.display_name, Some(user.id)).await?;
    state.events.publish(crate::events::EventMsg { team_id, entity: "team".into(), id: team_id, version: 0, action: "member_left".into(), actor: user.display_name.clone() });
    // devices that were on this team move on
    let next: Option<i64> = sqlx::query("SELECT team_id FROM memberships WHERE user_id = ? ORDER BY created_at LIMIT 1")
        .bind(user.id).fetch_optional(&state.db).await?.map(|r| r.get("team_id"));
    sqlx::query("UPDATE sessions SET team_id = ? WHERE user_id = ? AND team_id = ?").bind(next).bind(user.id).bind(team_id).execute(&state.dbw).await?;
    if let Some(n) = next {
        sqlx::query("UPDATE users SET team_id = ? WHERE id = ? AND team_id = ?").bind(n).bind(user.id).bind(team_id).execute(&state.dbw).await?;
    }
    Ok(Json(json!({ "ok": true, "left": team_id, "active_team": next })))
}

/// GET /teams/{id} — everything the Teams page shows when a card is
/// opened: stammdaten, my role, roster counts, members; the join code only
/// for a coach of that team.
pub async fn get_team(State(state): State<AppState>, user: CurrentUser, Path(team_id): Path<i64>) -> ApiResult<Json<Value>> {
    let m = sqlx::query("SELECT role FROM memberships WHERE user_id = ? AND team_id = ?")
        .bind(user.id).bind(team_id).fetch_optional(&state.db).await?
        .ok_or(ApiError::Forbidden)?;
    let my_role = Role::parse(&m.get::<String, _>("role"));
    let t = sqlx::query("SELECT id, name, short, league, season, join_code FROM teams WHERE id = ?")
        .bind(team_id).fetch_one(&state.db).await?;
    let mut positions = json!({ "Z": 0, "A": 0, "M": 0, "D": 0, "L": 0 });
    let mut active = 0i64;
    let mut inactive = 0i64;
    for r in sqlx::query("SELECT position, active, count(*) AS n FROM players WHERE team_id = ? GROUP BY position, active").bind(team_id).fetch_all(&state.db).await? {
        let n: i64 = r.get("n");
        if r.get::<i64, _>("active") != 0 {
            active += n;
            let p: String = r.get("position");
            positions[p] = json!(positions[&p].as_i64().unwrap_or(0) + n);
        } else {
            inactive += n;
        }
    }
    let members: Vec<Value> = sqlx::query(
        "SELECT u.id, u.username, u.display_name, m.role FROM memberships m JOIN users u ON u.id = m.user_id
         WHERE m.team_id = ? ORDER BY CASE m.role WHEN 'coach' THEN 0 WHEN 'assistant' THEN 1 ELSE 2 END, u.display_name",
    )
    .bind(team_id).fetch_all(&state.db).await?
    .iter()
    .map(|r| json!({ "id": r.get::<i64, _>("id"), "username": r.get::<String, _>("username"), "display_name": r.get::<String, _>("display_name"), "role": r.get::<String, _>("role") }))
    .collect();
    Ok(Json(json!({
        "id": t.get::<i64, _>("id"),
        "name": t.get::<String, _>("name"),
        "short": t.get::<String, _>("short"),
        "league": t.get::<String, _>("league"),
        "season": t.get::<String, _>("season"),
        "role": my_role.as_str(),
        "player_count": active,
        "inactive_count": inactive,
        "positions": positions,
        "member_count": members.len(),
        "members": members,
        "join_code": if my_role == Role::Coach { Value::String(t.get::<String, _>("join_code")) } else { Value::Null },
    })))
}
