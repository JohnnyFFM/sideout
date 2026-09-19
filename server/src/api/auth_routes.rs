//! Signup (found a team), join by team code, login, logout, me.

use axum::extract::State;
use axum::http::{header, HeaderMap};
use axum::response::IntoResponse;
use axum::Json;
use rand::Rng;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;

use crate::auth::{self, CurrentUser, Role};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::store::audit;

fn validate_credentials(username: &str, password: &str, display: &str) -> ApiResult<()> {
    if username.len() < 3 {
        return Err(ApiError::BadRequest("Benutzername zu kurz (min. 3 Zeichen)".into()));
    }
    if password.len() < 8 {
        return Err(ApiError::BadRequest("Passwort zu kurz (min. 8 Zeichen)".into()));
    }
    if display.trim().is_empty() {
        return Err(ApiError::BadRequest("Anzeigename fehlt".into()));
    }
    Ok(())
}

pub fn new_join_code() -> String {
    let mut rng = rand::thread_rng();
    let letters: String = (0..3).map(|_| (b'A' + rng.gen_range(0..26)) as char).collect();
    format!("{letters}-{:04}", rng.gen_range(0..10000))
}

async fn start_session(state: &AppState, user_id: i64, headers: &HeaderMap) -> ApiResult<String> {
    let token = auth::new_token();
    let device = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|ua| {
            let l = ua.to_lowercase();
            if l.contains("iphone") { "iPhone" }
            else if l.contains("ipad") { "iPad" }
            else if l.contains("android") { "Android" }
            else if l.contains("windows") { "Windows" }
            else if l.contains("mac") { "Mac" }
            else { "Gerät" }
        })
        .unwrap_or("Gerät");
    sqlx::query("INSERT INTO sessions (user_id, token_hash, device_label) VALUES (?, ?, ?)")
        .bind(user_id)
        .bind(auth::token_hash(&token))
        .bind(device)
        .execute(&state.dbw)
        .await?;
    Ok(token)
}

fn secure(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .map(|p| p == "https")
        .unwrap_or(false)
}

pub async fn me_payload(state: &AppState, user: &CurrentUser) -> ApiResult<Value> {
    let team = sqlx::query("SELECT id, name, short, league, season, join_code FROM teams WHERE id = ?")
        .bind(user.team_id)
        .fetch_one(&state.db)
        .await?;
    let members: Vec<Value> = sqlx::query(
        "SELECT id, username, display_name, role FROM users WHERE team_id = ? ORDER BY id",
    )
    .bind(user.team_id)
    .fetch_all(&state.db)
    .await?
    .iter()
    .map(|r| {
        json!({
            "id": r.get::<i64, _>("id"),
            "username": r.get::<String, _>("username"),
            "display_name": r.get::<String, _>("display_name"),
            "role": r.get::<String, _>("role"),
        })
    })
    .collect();
    Ok(json!({
        "user": {
            "id": user.id,
            "username": user.username,
            "display_name": user.display_name,
            "role": user.role.as_str(),
        },
        "team": {
            "id": team.get::<i64, _>("id"),
            "name": team.get::<String, _>("name"),
            "short": team.get::<String, _>("short"),
            "league": team.get::<String, _>("league"),
            "season": team.get::<String, _>("season"),
            "join_code": if user.role == Role::Coach { Value::String(team.get::<String, _>("join_code")) } else { Value::Null },
        },
        "members": members,
    }))
}

#[derive(Deserialize)]
pub struct RegisterTeam {
    pub team_name: String,
    pub display_name: String,
    pub username: String,
    pub password: String,
}

pub async fn config(State(state): State<AppState>) -> Json<Value> {
    Json(json!({ "signup_open": state.config.signup_open }))
}

fn short_of(name: &str) -> String {
    let s: String = name.split_whitespace().filter_map(|w| w.chars().next()).take(3).collect();
    let s = s.to_uppercase();
    if s.len() >= 2 { s } else { name.chars().take(3).collect::<String>().to_uppercase() }
}

pub async fn register_team(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<RegisterTeam>,
) -> ApiResult<impl IntoResponse> {
    if !state.config.signup_open {
        return Err(ApiError::BadRequest("Registrierung ist geschlossen".into()));
    }
    let username = body.username.trim().to_lowercase();
    validate_credentials(&username, &body.password, &body.display_name)?;
    if body.team_name.trim().is_empty() {
        return Err(ApiError::BadRequest("Teamname fehlt".into()));
    }
    let hash = auth::hash_password(&body.password).map_err(ApiError::Internal)?;

    let mut team_id = 0i64;
    for _ in 0..5 {
        let code = new_join_code();
        match sqlx::query("INSERT INTO teams (name, short, join_code) VALUES (?, ?, ?)")
            .bind(body.team_name.trim())
            .bind(short_of(body.team_name.trim()))
            .bind(&code)
            .execute(&state.dbw)
            .await
        {
            Ok(res) => { team_id = res.last_insert_rowid(); break; }
            Err(sqlx::Error::Database(e)) if e.is_unique_violation() => continue,
            Err(e) => return Err(e.into()),
        }
    }
    if team_id == 0 {
        return Err(ApiError::Internal("join code generation failed".into()));
    }
    let res = sqlx::query(
        "INSERT INTO users (team_id, username, display_name, password_hash, role) VALUES (?, ?, ?, ?, 'coach')",
    )
    .bind(team_id)
    .bind(&username)
    .bind(body.display_name.trim())
    .bind(&hash)
    .execute(&state.dbw)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref d) if d.is_unique_violation() => {
            ApiError::BadRequest("Benutzername ist bereits vergeben".into())
        }
        other => other.into(),
    })?;
    let user_id = res.last_insert_rowid();
    audit(&state, team_id, "team", team_id, "created", "Team angelegt", Some(user_id)).await?;

    let token = start_session(&state, user_id, &headers).await?;
    let user = auth::validate_session(&state.db, &state.dbw, &token).await?;
    let payload = me_payload(&state, &user).await?;
    Ok(([(header::SET_COOKIE, auth::session_cookie(&token, secure(&headers)))], Json(payload)))
}

#[derive(Deserialize)]
pub struct JoinTeam {
    pub code: String,
    pub display_name: String,
    pub username: String,
    pub password: String,
}

pub async fn join(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<JoinTeam>,
) -> ApiResult<impl IntoResponse> {
    if !state.config.signup_open {
        return Err(ApiError::BadRequest("Registrierung ist geschlossen".into()));
    }
    let username = body.username.trim().to_lowercase();
    validate_credentials(&username, &body.password, &body.display_name)?;
    let team = sqlx::query("SELECT id FROM teams WHERE join_code = ?")
        .bind(body.code.trim().to_uppercase())
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| ApiError::BadRequest("Team-Code nicht gefunden".into()))?;
    let team_id: i64 = team.get("id");
    let hash = auth::hash_password(&body.password).map_err(ApiError::Internal)?;
    let res = sqlx::query(
        "INSERT INTO users (team_id, username, display_name, password_hash, role) VALUES (?, ?, ?, ?, 'assistant')",
    )
    .bind(team_id)
    .bind(&username)
    .bind(body.display_name.trim())
    .bind(&hash)
    .execute(&state.dbw)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref d) if d.is_unique_violation() => {
            ApiError::BadRequest("Benutzername ist bereits vergeben".into())
        }
        other => other.into(),
    })?;
    let user_id = res.last_insert_rowid();
    audit(&state, team_id, "team", team_id, "member_joined", body.display_name.trim(), Some(user_id)).await?;
    state.events.publish(crate::events::EventMsg {
        team_id, entity: "team".into(), id: team_id, version: 0,
        action: "member_joined".into(), actor: body.display_name.trim().to_string(),
    });
    let token = start_session(&state, user_id, &headers).await?;
    let user = auth::validate_session(&state.db, &state.dbw, &token).await?;
    let payload = me_payload(&state, &user).await?;
    Ok(([(header::SET_COOKIE, auth::session_cookie(&token, secure(&headers)))], Json(payload)))
}

#[derive(Deserialize)]
pub struct Login {
    pub username: String,
    pub password: String,
}

pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Login>,
) -> ApiResult<impl IntoResponse> {
    let row = sqlx::query("SELECT id, password_hash FROM users WHERE username = ?")
        .bind(body.username.trim().to_lowercase())
        .fetch_optional(&state.db)
        .await?;
    let (user_id, hash): (i64, String) = match row {
        Some(r) => (r.get("id"), r.get("password_hash")),
        None => return Err(ApiError::BadRequest("Anmeldung fehlgeschlagen".into())),
    };
    if !auth::verify_password(&body.password, &hash) {
        return Err(ApiError::BadRequest("Anmeldung fehlgeschlagen".into()));
    }
    let token = start_session(&state, user_id, &headers).await?;
    let user = auth::validate_session(&state.db, &state.dbw, &token).await?;
    let payload = me_payload(&state, &user).await?;
    Ok(([(header::SET_COOKIE, auth::session_cookie(&token, secure(&headers)))], Json(payload)))
}

pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<impl IntoResponse> {
    if let Some(token) = auth::cookie_from_headers(&headers) {
        sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
            .bind(auth::token_hash(&token))
            .execute(&state.dbw)
            .await
            .ok();
    }
    Ok(([(header::SET_COOKIE, auth::clear_cookie())], Json(json!({"ok": true}))))
}

pub async fn me(State(state): State<AppState>, user: CurrentUser) -> ApiResult<Json<Value>> {
    Ok(Json(me_payload(&state, &user).await?))
}
