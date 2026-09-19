//! Self-contained auth: argon2id passwords, 12-month rolling device sessions
//! (HttpOnly cookie, token stored sha256-hashed), CSRF via the X-Requested-By
//! header. Any handler taking `user: CurrentUser` is authenticated by
//! construction; mutating handlers call `user.require(Role::Assistant)?`.

use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{header, HeaderMap, Method};
use axum::response::IntoResponse;
use base64::Engine;
use rand::RngCore;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub const SESSION_DAYS: i64 = 365;
pub const COOKIE: &str = "so_session";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Viewer,
    Assistant,
    Coach,
}

impl Role {
    pub fn parse(s: &str) -> Role {
        match s {
            "coach" => Role::Coach,
            "viewer" => Role::Viewer,
            _ => Role::Assistant,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Coach => "coach",
            Role::Assistant => "assistant",
            Role::Viewer => "viewer",
        }
    }
}

#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub id: i64,
    pub team_id: i64,
    pub username: String,
    pub display_name: String,
    pub role: Role,
}

impl CurrentUser {
    pub fn require(&self, min: Role) -> ApiResult<()> {
        if self.role >= min {
            Ok(())
        } else {
            Err(ApiError::Forbidden)
        }
    }
}

pub fn hash_password(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| e.to_string())
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|parsed| Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
        .unwrap_or(false)
}

pub fn new_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub fn token_hash(token: &str) -> String {
    let mut h = Sha256::new();
    h.update(token.as_bytes());
    format!("{:x}", h.finalize())
}

pub fn cookie_from_headers(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    raw.split(';')
        .map(str::trim)
        .find(|c| c.starts_with(&format!("{COOKIE}=")))
        .map(|c| c[COOKIE.len() + 1..].to_string())
}

/// `Secure` only behind https (Caddy sets x-forwarded-proto) — locally the
/// plain cookie must work.
pub fn session_cookie(token: &str, secure: bool) -> String {
    let base = format!(
        "{COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}",
        SESSION_DAYS * 86400
    );
    if secure {
        format!("{base}; Secure")
    } else {
        base
    }
}

pub fn clear_cookie() -> String {
    format!("{COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0")
}

pub async fn validate_session(db: &SqlitePool, dbw: &SqlitePool, token: &str) -> ApiResult<CurrentUser> {
    let th = token_hash(token);
    let row = sqlx::query(
        "SELECT s.id AS sid, s.last_seen, u.id, u.team_id, u.username, u.display_name, u.role
         FROM sessions s JOIN users u ON u.id = s.user_id
         WHERE s.token_hash = ? AND s.last_seen > datetime('now', ?)",
    )
    .bind(&th)
    .bind(format!("-{SESSION_DAYS} days"))
    .fetch_optional(db)
    .await?
    .ok_or(ApiError::Unauthorized)?;

    // rolling session: refresh last_seen, throttled to ~hourly
    let sid: i64 = row.get("sid");
    let last_seen: String = row.get("last_seen");
    let stale = sqlx::query("SELECT datetime('now', '-1 hour') > ? AS stale")
        .bind(&last_seen)
        .fetch_one(db)
        .await?
        .get::<i64, _>("stale")
        != 0;
    if stale {
        sqlx::query("UPDATE sessions SET last_seen = datetime('now') WHERE id = ?")
            .bind(sid)
            .execute(dbw)
            .await
            .ok();
    }

    Ok(CurrentUser {
        id: row.get("id"),
        team_id: row.get("team_id"),
        username: row.get("username"),
        display_name: row.get("display_name"),
        role: Role::parse(row.get::<String, _>("role").as_str()),
    })
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let token = cookie_from_headers(&parts.headers).ok_or(ApiError::Unauthorized)?;
        validate_session(&state.db, &state.dbw, &token).await
    }
}

/// Router-wide CSRF guard: any non-GET request must carry X-Requested-By
/// (the SPA sets it on every call). Sufficient with SameSite=Lax cookies.
pub async fn csrf_layer(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let m = req.method();
    if m != Method::GET && m != Method::HEAD && m != Method::OPTIONS
        && !req.headers().contains_key("x-requested-by")
    {
        return ApiError::Forbidden.into_response();
    }
    next.run(req).await
}
