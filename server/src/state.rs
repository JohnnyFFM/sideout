use std::path::PathBuf;
use std::sync::Arc;

use sqlx::SqlitePool;

use crate::events::EventBus;

pub struct Config {
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub bind: String,
    pub web_dir: PathBuf,
    /// SO_SIGNUP=closed gates register-team AND join-by-code — accounts are
    /// then seeded via the CLI only (the gated-production mode).
    pub signup_open: bool,
    /// SO_BASE: path prefix the whole app is mounted under (e.g. "/sideout"
    /// behind a shared reverse proxy). Must match the web build's SO_BASE.
    pub base: String,
}

impl Config {
    pub fn from_env() -> Self {
        let data_dir = PathBuf::from(std::env::var("SO_DATA").unwrap_or_else(|_| "./data".into()));
        let db_path = std::env::var("SO_DB")
            .map(PathBuf::from)
            .unwrap_or_else(|_| data_dir.join("sideout.db"));
        Self {
            data_dir,
            db_path,
            bind: std::env::var("SO_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".into()),
            web_dir: PathBuf::from(
                std::env::var("SO_STATIC").unwrap_or_else(|_| "../web/build".into()),
            ),
            signup_open: std::env::var("SO_SIGNUP").map(|v| v != "closed").unwrap_or(true),
            base: std::env::var("SO_BASE").map(|b| b.trim_end_matches('/').to_string()).unwrap_or_default(),
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    /// Read pool — several connections, WAL readers don't block.
    pub db: SqlitePool,
    /// Write pool — exactly one connection: all writes serialize by construction.
    pub dbw: SqlitePool,
    pub events: EventBus,
    pub config: Arc<Config>,
}
