mod api;
mod auth;
mod engine;
mod error;
mod events;
mod state;
mod store;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use tower_http::compression::CompressionLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use crate::state::{AppState, Config};

/// Hashed build assets are immutable, everything else (app shell, service
/// worker, manifest) must revalidate every time so phones never pin a
/// stale bundle.
async fn cache_policy(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let immutable = req.uri().path().contains("/_app/immutable/");
    let mut res = next.run(req).await;
    let hv = if immutable {
        axum::http::HeaderValue::from_static("public, max-age=31536000, immutable")
    } else {
        axum::http::HeaderValue::from_static("no-cache")
    };
    res.headers_mut().insert(axum::http::header::CACHE_CONTROL, hv);
    res
}

/// PWA manifest, served host-aware: dev/test installs get a name suffix so
/// they can share a home screen with the live app.
async fn serve_manifest(path: std::path::PathBuf, headers: axum::http::HeaderMap) -> axum::response::Response {
    use axum::response::IntoResponse;
    let raw = match tokio::fs::read_to_string(&path).await {
        Ok(r) => r,
        Err(_) => return axum::http::StatusCode::NOT_FOUND.into_response(),
    };
    let host = headers
        .get(axum::http::header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("");
    let suffix = if host == "localhost" || host.starts_with("127.") {
        Some(" (Dev)")
    } else if host.starts_with("test.") {
        Some(" (Test)")
    } else {
        None
    };
    let body = match suffix {
        Some(sfx) => match serde_json::from_str::<serde_json::Value>(&raw) {
            Ok(mut v) => {
                for k in ["name", "short_name"] {
                    if let Some(s) = v.get(k).and_then(|x| x.as_str()).map(str::to_string) {
                        v[k] = serde_json::Value::String(format!("{s}{sfx}"));
                    }
                }
                v.to_string()
            }
            Err(_) => raw,
        },
        None => raw,
    };
    ([(axum::http::header::CONTENT_TYPE, "application/manifest+json")], body).into_response()
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sideout_server=info,tower_http=warn".into()),
        )
        .init();

    let config = Config::from_env();
    std::fs::create_dir_all(&config.data_dir).expect("create data dir");

    let opts = SqliteConnectOptions::new()
        .filename(&config.db_path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_millis(5000))
        .foreign_keys(true);

    let dbw = SqlitePoolOptions::new().max_connections(1).connect_with(opts.clone()).await.expect("open db (write)");
    let db = SqlitePoolOptions::new().max_connections(4).connect_with(opts).await.expect("open db (read)");

    sqlx::migrate!("./migrations").run(&dbw).await.expect("run migrations");

    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        run_cli(&args[1..], &dbw).await;
        return;
    }

    let state = AppState { db, dbw, events: events::EventBus::new(), config: Arc::new(config) };

    let spa = ServeDir::new(&state.config.web_dir)
        .fallback(ServeFile::new(state.config.web_dir.join("index.html")));
    let manifest_route = {
        let path = state.config.web_dir.join("manifest.webmanifest");
        axum::routing::get(move |headers: axum::http::HeaderMap| serve_manifest(path.clone(), headers))
    };

    let app = axum::Router::new()
        .nest("/api", api::router(state.clone()))
        .route("/manifest.webmanifest", manifest_route)
        .fallback_service(spa)
        .layer(axum::middleware::from_fn(cache_policy))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http());

    let bind = state.config.bind.clone();
    let listener = tokio::net::TcpListener::bind(&bind).await.expect("bind");
    tracing::info!("sideout listening on http://{bind}");
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .expect("server");
}

fn cli_password(username: &str) -> String {
    match std::env::var("SO_PASSWORD") {
        Ok(p) if !p.is_empty() => p,
        _ => rpassword::prompt_password(format!("Password for {username}: ")).expect("read password"),
    }
}

/// Account seeding for gated production (SO_SIGNUP=closed).
async fn run_cli(args: &[String], db: &sqlx::SqlitePool) {
    use sqlx::Row;
    match args[0].as_str() {
        "team-add" => {
            if args.len() < 2 {
                eprintln!("usage: sideout-server team-add <name>");
                std::process::exit(2);
            }
            for _ in 0..5 {
                let code = api::auth_routes::new_join_code();
                match sqlx::query("INSERT INTO teams (name, join_code) VALUES (?, ?)")
                    .bind(args[1].trim())
                    .bind(&code)
                    .execute(db)
                    .await
                {
                    Ok(res) => {
                        println!("team '{}' created: id {} · join code {code}", args[1].trim(), res.last_insert_rowid());
                        return;
                    }
                    Err(sqlx::Error::Database(e)) if e.is_unique_violation() => continue,
                    Err(e) => { eprintln!("failed: {e}"); std::process::exit(1); }
                }
            }
            eprintln!("join code generation failed");
            std::process::exit(1);
        }
        "user-add" => {
            if args.len() < 4 {
                eprintln!("usage: sideout-server user-add <team_id> <username> <display_name> [role]");
                eprintln!("       role: coach | assistant | viewer (default assistant)");
                eprintln!("       password read from SO_PASSWORD or prompted");
                std::process::exit(2);
            }
            let team_id: i64 = args[1].parse().expect("team_id must be a number");
            let username = args[2].trim().to_lowercase();
            let display = args[3].trim().to_string();
            let role = args.get(4).map(|s| s.as_str()).unwrap_or("assistant");
            if !matches!(role, "coach" | "assistant" | "viewer") {
                eprintln!("role must be coach | assistant | viewer");
                std::process::exit(2);
            }
            let password = cli_password(&username);
            if password.len() < 8 {
                eprintln!("password must be at least 8 characters");
                std::process::exit(2);
            }
            let hash = auth::hash_password(&password).expect("hash password");
            match sqlx::query("INSERT INTO users (team_id, username, display_name, password_hash, role) VALUES (?, ?, ?, ?, ?)")
                .bind(team_id).bind(&username).bind(&display).bind(&hash).bind(role)
                .execute(db)
                .await
            {
                Ok(_) => println!("user '{username}' ({role}) created in team {team_id}"),
                Err(e) => { eprintln!("failed: {e}"); std::process::exit(1); }
            }
        }
        "user-passwd" => {
            if args.len() < 2 {
                eprintln!("usage: sideout-server user-passwd <username>");
                std::process::exit(2);
            }
            let username = args[1].trim().to_lowercase();
            let password = cli_password(&username);
            let hash = auth::hash_password(&password).expect("hash password");
            let res = sqlx::query("UPDATE users SET password_hash = ? WHERE username = ?")
                .bind(&hash).bind(&username).execute(db).await.expect("update");
            if res.rows_affected() == 0 {
                eprintln!("no such user");
                std::process::exit(1);
            }
            sqlx::query("DELETE FROM sessions WHERE user_id = (SELECT id FROM users WHERE username = ?)")
                .bind(&username).execute(db).await.ok();
            println!("password updated, sessions revoked");
        }
        "user-list" => {
            let rows = sqlx::query(
                "SELECT u.username, u.display_name, u.role, t.name AS team FROM users u JOIN teams t ON t.id = u.team_id ORDER BY u.id",
            )
            .fetch_all(db)
            .await
            .expect("query");
            for r in rows {
                println!(
                    "{:<16} {:<20} {:<10} {}",
                    r.get::<String, _>("username"),
                    r.get::<String, _>("display_name"),
                    r.get::<String, _>("role"),
                    r.get::<String, _>("team")
                );
            }
        }
        other => {
            eprintln!("unknown command '{other}'. Commands: team-add, user-add, user-passwd, user-list");
            std::process::exit(2);
        }
    }
}
