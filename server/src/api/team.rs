//! Team settings, members and the roster (players).

use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;

use crate::auth::{CurrentUser, Role};
use crate::error::{ApiError, ApiResult};
use crate::events::EventMsg;
use crate::state::AppState;
use crate::store::{audit, player_json};

use super::auth_routes::{me_payload, new_join_code};

#[derive(Deserialize)]
pub struct PatchTeam {
    pub name: Option<String>,
    pub short: Option<String>,
    pub league: Option<String>,
    pub season: Option<String>,
}

pub async fn patch_team(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(body): Json<PatchTeam>,
) -> ApiResult<Json<Value>> {
    user.require(Role::Coach)?;
    if let Some(n) = &body.name {
        if n.trim().is_empty() {
            return Err(ApiError::BadRequest("Teamname fehlt".into()));
        }
    }
    sqlx::query(
        "UPDATE teams SET name = COALESCE(?, name), short = COALESCE(?, short),
         league = COALESCE(?, league), season = COALESCE(?, season) WHERE id = ?",
    )
    .bind(body.name.as_deref().map(str::trim))
    .bind(body.short.as_deref().map(|s| s.trim().to_uppercase()))
    .bind(body.league.as_deref().map(str::trim))
    .bind(body.season.as_deref().map(str::trim))
    .bind(user.team_id)
    .execute(&state.dbw)
    .await?;
    audit(&state, user.team_id, "team", user.team_id, "updated", "", Some(user.id)).await?;
    state.events.publish(EventMsg { team_id: user.team_id, entity: "team".into(), id: user.team_id, version: 0, action: "updated".into(), actor: user.display_name.clone() });
    Ok(Json(me_payload(&state, &user).await?))
}

pub async fn rotate_code(State(state): State<AppState>, user: CurrentUser) -> ApiResult<Json<Value>> {
    user.require(Role::Coach)?;
    for _ in 0..5 {
        let code = new_join_code();
        match sqlx::query("UPDATE teams SET join_code = ? WHERE id = ?")
            .bind(&code)
            .bind(user.team_id)
            .execute(&state.dbw)
            .await
        {
            Ok(_) => return Ok(Json(json!({ "join_code": code }))),
            Err(sqlx::Error::Database(e)) if e.is_unique_violation() => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(ApiError::Internal("join code generation failed".into()))
}

#[derive(Deserialize)]
pub struct PatchMember {
    pub display_name: Option<String>,
    pub role: Option<String>,
}

pub async fn patch_member(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(body): Json<PatchMember>,
) -> ApiResult<Json<Value>> {
    user.require(Role::Coach)?;
    if let Some(r) = &body.role {
        if !matches!(r.as_str(), "coach" | "assistant" | "viewer") {
            return Err(ApiError::BadRequest("Rolle unbekannt".into()));
        }
        if id == user.id && r != "coach" {
            return Err(ApiError::BadRequest("Die eigene Trainer-Rolle kann nicht abgegeben werden".into()));
        }
    }
    let res = sqlx::query("UPDATE memberships SET role = COALESCE(?, role) WHERE user_id = ? AND team_id = ?")
        .bind(body.role.as_deref())
        .bind(id)
        .bind(user.team_id)
        .execute(&state.dbw)
        .await?;
    if res.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    if let Some(n) = body.display_name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        sqlx::query("UPDATE users SET display_name = ? WHERE id = ?").bind(n).bind(id).execute(&state.dbw).await?;
    }
    Ok(Json(me_payload(&state, &user).await?))
}

pub async fn delete_member(State(state): State<AppState>, user: CurrentUser, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    user.require(Role::Coach)?;
    if id == user.id {
        return Err(ApiError::BadRequest("Du kannst dich nicht selbst entfernen".into()));
    }
    let res = sqlx::query("DELETE FROM memberships WHERE user_id = ? AND team_id = ?")
        .bind(id)
        .bind(user.team_id)
        .execute(&state.dbw)
        .await?;
    if res.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    // the removed member's selected team moves to another membership, if any
    sqlx::query(
        "UPDATE users SET team_id = COALESCE((SELECT team_id FROM memberships WHERE user_id = ? ORDER BY created_at LIMIT 1), team_id)
         WHERE id = ?",
    )
    .bind(id).bind(id).execute(&state.dbw).await?;
    Ok(Json(me_payload(&state, &user).await?))
}

// ---------------------------------------------------------------- my teams

#[derive(Deserialize)]
pub struct NewTeam {
    pub name: String,
}

/// A logged-in user founds another team and becomes its coach.
pub async fn create_team(State(state): State<AppState>, user: CurrentUser, Json(body): Json<NewTeam>) -> ApiResult<Json<Value>> {
    if body.name.trim().is_empty() {
        return Err(ApiError::BadRequest("Teamname fehlt".into()));
    }
    let short: String = body.name.split_whitespace().filter_map(|w| w.chars().next()).take(3).collect::<String>().to_uppercase();
    let mut team_id = 0i64;
    for _ in 0..5 {
        let code = new_join_code();
        match sqlx::query("INSERT INTO teams (name, short, join_code) VALUES (?, ?, ?)")
            .bind(body.name.trim()).bind(&short).bind(&code).execute(&state.dbw).await
        {
            Ok(res) => { team_id = res.last_insert_rowid(); break; }
            Err(sqlx::Error::Database(e)) if e.is_unique_violation() => continue,
            Err(e) => return Err(e.into()),
        }
    }
    if team_id == 0 {
        return Err(ApiError::Internal("join code generation failed".into()));
    }
    sqlx::query("INSERT INTO memberships (user_id, team_id, role) VALUES (?, ?, 'coach')").bind(user.id).bind(team_id).execute(&state.dbw).await?;
    sqlx::query("UPDATE users SET team_id = ? WHERE id = ?").bind(team_id).bind(user.id).execute(&state.dbw).await?;
    audit(&state, team_id, "team", team_id, "created", "Team angelegt", Some(user.id)).await?;
    let switched = CurrentUser { team_id, role: Role::Coach, ..user };
    Ok(Json(me_payload(&state, &switched).await?))
}

#[derive(Deserialize)]
pub struct JoinCode {
    pub code: String,
}

/// A logged-in user joins another team by code (as assistant) and switches to it.
pub async fn join_team(State(state): State<AppState>, user: CurrentUser, Json(body): Json<JoinCode>) -> ApiResult<Json<Value>> {
    if !state.config.signup_open {
        return Err(ApiError::BadRequest("Beitreten ist auf dieser Instanz geschlossen".into()));
    }
    let team = sqlx::query("SELECT id FROM teams WHERE join_code = ?")
        .bind(body.code.trim().to_uppercase())
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| ApiError::BadRequest("Team-Code nicht gefunden".into()))?;
    let team_id: i64 = team.get("id");
    sqlx::query("INSERT OR IGNORE INTO memberships (user_id, team_id, role) VALUES (?, ?, 'assistant')").bind(user.id).bind(team_id).execute(&state.dbw).await?;
    sqlx::query("UPDATE users SET team_id = ? WHERE id = ?").bind(team_id).bind(user.id).execute(&state.dbw).await?;
    audit(&state, team_id, "team", team_id, "member_joined", &user.display_name, Some(user.id)).await?;
    state.events.publish(EventMsg { team_id, entity: "team".into(), id: team_id, version: 0, action: "member_joined".into(), actor: user.display_name.clone() });
    let role: String = sqlx::query("SELECT role FROM memberships WHERE user_id = ? AND team_id = ?").bind(user.id).bind(team_id).fetch_one(&state.db).await?.get("role");
    let switched = CurrentUser { team_id, role: Role::parse(&role), ..user };
    Ok(Json(me_payload(&state, &switched).await?))
}

#[derive(Deserialize)]
pub struct SwitchTeam {
    pub team_id: i64,
}

pub async fn switch_team(State(state): State<AppState>, user: CurrentUser, Json(body): Json<SwitchTeam>) -> ApiResult<Json<Value>> {
    let m = sqlx::query("SELECT role FROM memberships WHERE user_id = ? AND team_id = ?")
        .bind(user.id).bind(body.team_id).fetch_optional(&state.db).await?
        .ok_or(ApiError::Forbidden)?;
    sqlx::query("UPDATE users SET team_id = ? WHERE id = ?").bind(body.team_id).bind(user.id).execute(&state.dbw).await?;
    let switched = CurrentUser { team_id: body.team_id, role: Role::parse(&m.get::<String, _>("role")), ..user };
    Ok(Json(me_payload(&state, &switched).await?))
}

// ---------------------------------------------------------------- players

pub async fn list_players(State(state): State<AppState>, user: CurrentUser) -> ApiResult<Json<Value>> {
    let rows = sqlx::query("SELECT * FROM players WHERE team_id = ? ORDER BY active DESC, number")
        .bind(user.team_id)
        .fetch_all(&state.db)
        .await?;
    Ok(Json(json!({ "players": rows.iter().map(player_json).collect::<Vec<_>>() })))
}

#[derive(Deserialize)]
pub struct NewPlayer {
    pub number: i64,
    pub name: String,
    pub position: String,
}

fn validate_player(number: i64, name: &str, position: &str) -> ApiResult<()> {
    if !(0..=99).contains(&number) {
        return Err(ApiError::BadRequest("Nummer muss zwischen 0 und 99 liegen".into()));
    }
    if name.trim().is_empty() {
        return Err(ApiError::BadRequest("Name fehlt".into()));
    }
    if !matches!(position, "Z" | "A" | "M" | "D" | "L") {
        return Err(ApiError::BadRequest("Position muss Z, A, M, D oder L sein".into()));
    }
    Ok(())
}

pub async fn create_player(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(body): Json<NewPlayer>,
) -> ApiResult<Json<Value>> {
    user.require(Role::Assistant)?;
    validate_player(body.number, &body.name, &body.position)?;
    let res = sqlx::query("INSERT INTO players (team_id, number, name, position) VALUES (?, ?, ?, ?)")
        .bind(user.team_id)
        .bind(body.number)
        .bind(body.name.trim())
        .bind(&body.position)
        .execute(&state.dbw)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref d) if d.is_unique_violation() => {
                ApiError::BadRequest(format!("Nummer {} ist schon vergeben", body.number))
            }
            other => other.into(),
        })?;
    let id = res.last_insert_rowid();
    audit(&state, user.team_id, "player", id, "created", body.name.trim(), Some(user.id)).await?;
    state.events.publish(EventMsg { team_id: user.team_id, entity: "player".into(), id, version: 0, action: "created".into(), actor: user.display_name.clone() });
    let row = sqlx::query("SELECT * FROM players WHERE id = ?").bind(id).fetch_one(&state.db).await?;
    Ok(Json(player_json(&row)))
}

#[derive(Deserialize)]
pub struct PatchPlayer {
    pub number: Option<i64>,
    pub name: Option<String>,
    pub position: Option<String>,
    pub active: Option<bool>,
}

pub async fn patch_player(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(body): Json<PatchPlayer>,
) -> ApiResult<Json<Value>> {
    user.require(Role::Assistant)?;
    let cur = sqlx::query("SELECT * FROM players WHERE id = ? AND team_id = ?")
        .bind(id)
        .bind(user.team_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(ApiError::NotFound)?;
    let number = body.number.unwrap_or(cur.get("number"));
    let name = body.name.clone().unwrap_or(cur.get("name"));
    let position = body.position.clone().unwrap_or(cur.get("position"));
    validate_player(number, &name, &position)?;
    sqlx::query("UPDATE players SET number = ?, name = ?, position = ?, active = ? WHERE id = ?")
        .bind(number)
        .bind(name.trim())
        .bind(&position)
        .bind(body.active.map(|b| b as i64).unwrap_or(cur.get("active")))
        .bind(id)
        .execute(&state.dbw)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref d) if d.is_unique_violation() => {
                ApiError::BadRequest(format!("Nummer {number} ist schon vergeben"))
            }
            other => other.into(),
        })?;
    audit(&state, user.team_id, "player", id, "updated", name.trim(), Some(user.id)).await?;
    state.events.publish(EventMsg { team_id: user.team_id, entity: "player".into(), id, version: 0, action: "updated".into(), actor: user.display_name.clone() });
    let row = sqlx::query("SELECT * FROM players WHERE id = ?").bind(id).fetch_one(&state.db).await?;
    Ok(Json(player_json(&row)))
}

/// A player with scouted actions is never deleted (the log references her);
/// she is deactivated instead and keeps her history.
pub async fn delete_player(State(state): State<AppState>, user: CurrentUser, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    user.require(Role::Assistant)?;
    let owned = sqlx::query("SELECT id FROM players WHERE id = ? AND team_id = ?")
        .bind(id)
        .bind(user.team_id)
        .fetch_optional(&state.db)
        .await?;
    if owned.is_none() {
        return Err(ApiError::NotFound);
    }
    let used: i64 = sqlx::query(
        "SELECT (SELECT count(*) FROM actions WHERE player_id = ? OR sub_in = ? OR sub_out = ?)
              + (SELECT count(*) FROM lineups WHERE ? IN (pos1,pos2,pos3,pos4,pos5,pos6,libero_id)) AS n",
    )
    .bind(id).bind(id).bind(id).bind(id)
    .fetch_one(&state.db)
    .await?
    .get("n");
    if used > 0 {
        sqlx::query("UPDATE players SET active = 0 WHERE id = ?").bind(id).execute(&state.dbw).await?;
        audit(&state, user.team_id, "player", id, "deactivated", "", Some(user.id)).await?;
    } else {
        sqlx::query("DELETE FROM players WHERE id = ?").bind(id).execute(&state.dbw).await?;
        audit(&state, user.team_id, "player", id, "deleted", "", Some(user.id)).await?;
    }
    state.events.publish(EventMsg { team_id: user.team_id, entity: "player".into(), id, version: 0, action: "deleted".into(), actor: user.display_name.clone() });
    Ok(Json(json!({ "ok": true, "deactivated": used > 0 })))
}
