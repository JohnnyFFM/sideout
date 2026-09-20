//! API tests against a throw-away SQLite file: the router is driven in
//! process with tower's `oneshot`, cookies carried by hand.

use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use serde_json::{json, Value};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use tower::ServiceExt;

use crate::events::EventBus;
use crate::state::{AppState, Config};

struct App {
    router: Router,
    _dir: tempfile::TempDir,
}

async fn app() -> App {
    let dir = tempfile::tempdir().unwrap();
    let opts = SqliteConnectOptions::new()
        .filename(dir.path().join("t.db"))
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true);
    let dbw = SqlitePoolOptions::new().max_connections(1).connect_with(opts.clone()).await.unwrap();
    let db = SqlitePoolOptions::new().max_connections(4).connect_with(opts).await.unwrap();
    sqlx::migrate!("./migrations").run(&dbw).await.unwrap();
    let config = Config {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("t.db"),
        bind: String::new(),
        web_dir: dir.path().to_path_buf(),
        signup_open: true,
        base: String::new(),
    };
    let state = AppState { db, dbw, events: EventBus::new(), config: Arc::new(config) };
    App { router: super::router(state), _dir: dir }
}

/// (status, json body, Set-Cookie value if any)
async fn call(app: &App, method: Method, path: &str, cookie: Option<&str>, body: Option<Value>) -> (StatusCode, Value, Option<String>) {
    let mut req = Request::builder().method(method).uri(path).header("x-requested-by", "test");
    if let Some(c) = cookie {
        req = req.header(header::COOKIE, c);
    }
    let req = match body {
        Some(b) => req.header(header::CONTENT_TYPE, "application/json").body(Body::from(b.to_string())).unwrap(),
        None => req.body(Body::empty()).unwrap(),
    };
    let res = app.router.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let set_cookie = res
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.split(';').next().unwrap_or("").to_string());
    let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json, set_cookie)
}

async fn register(app: &App, team: &str, user: &str) -> (String, Value) {
    let (st, body, cookie) = call(
        app,
        Method::POST,
        "/auth/register-team",
        None,
        Some(json!({ "team_name": team, "display_name": user.to_uppercase(), "username": user, "password": "geheim123" })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    (cookie.unwrap(), body)
}

#[tokio::test]
async fn joining_by_code_gives_viewer_on_both_paths() {
    let app = app().await;
    let (coach, me) = register(&app, "Erste", "anna").await;
    let code = me["team"]["join_code"].as_str().unwrap().to_string();
    let team_id = me["team"]["id"].as_i64().unwrap();

    // 1. register with a code
    let (st, body, bob) = call(&app, Method::POST, "/auth/join", None, Some(json!({ "code": code, "display_name": "Bob", "username": "bob", "password": "geheim123" }))).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert_eq!(body["user"]["role"], "viewer");
    assert_eq!(body["teams"][0]["role"], "viewer");
    let bob = bob.unwrap();

    // 2. an existing account joins another team
    let (carl, _) = register(&app, "Zweite", "carl").await;
    let (st, body, _) = call(&app, Method::POST, "/teams/join", Some(&carl), Some(json!({ "code": code }))).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert_eq!(body["user"]["role"], "viewer");
    let mine: Vec<&str> = body["teams"].as_array().unwrap().iter().map(|t| t["role"].as_str().unwrap()).collect();
    assert!(mine.contains(&"viewer") && mine.contains(&"coach"));

    // viewers cannot write; the coach promotes
    let (st, _, _) = call(&app, Method::POST, "/players", Some(&bob), Some(json!({ "number": 7, "name": "X", "position": "A" }))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let bob_id = body["members"].as_array().unwrap().iter().find(|m| m["username"] == "bob").unwrap()["id"].as_i64().unwrap();
    let (st, _, _) = call(&app, Method::PATCH, &format!("/teams/{team_id}/members/{bob_id}"), Some(&coach), Some(json!({ "role": "assistant" }))).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _, _) = call(&app, Method::POST, "/players", Some(&bob), Some(json!({ "number": 7, "name": "X", "position": "A" }))).await;
    assert_eq!(st, StatusCode::OK);
}

#[tokio::test]
async fn empty_display_name_falls_back_to_the_username() {
    let app = app().await;
    let (c, _) = register(&app, "Erste", "anna").await;
    let (st, body, _) = call(&app, Method::PATCH, "/me", Some(&c), Some(json!({ "display_name": "Anna Berger" }))).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert_eq!(body["user"]["display_name"], "Anna Berger");
    let (st, body, _) = call(&app, Method::PATCH, "/me", Some(&c), Some(json!({ "display_name": "   " }))).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert_eq!(body["user"]["display_name"], "anna");
}

#[tokio::test]
async fn password_change_needs_the_current_one() {
    let app = app().await;
    let (c, _) = register(&app, "Erste", "anna").await;
    let (st, body, _) = call(&app, Method::POST, "/me/password", Some(&c), Some(json!({ "current": "falsch123", "new": "neuesgeheim" }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{body}");
    let (st, body, _) = call(&app, Method::POST, "/me/password", Some(&c), Some(json!({ "current": "geheim123", "new": "kurz" }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{body}");
    let (st, body, _) = call(&app, Method::POST, "/me/password", Some(&c), Some(json!({ "current": "geheim123", "new": "anna" }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{body}");
    let (st, body, _) = call(&app, Method::POST, "/me/password", Some(&c), Some(json!({ "current": "geheim123", "new": "neuesgeheim" }))).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let (st, _, _) = call(&app, Method::POST, "/auth/login", None, Some(json!({ "username": "anna", "password": "geheim123" }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _, _) = call(&app, Method::POST, "/auth/login", None, Some(json!({ "username": "anna", "password": "neuesgeheim" }))).await;
    assert_eq!(st, StatusCode::OK);
}

#[tokio::test]
async fn leaving_refuses_the_last_coach() {
    let app = app().await;
    let (coach, me) = register(&app, "Erste", "anna").await;
    let team_id = me["team"]["id"].as_i64().unwrap();
    let code = me["team"]["join_code"].as_str().unwrap().to_string();
    let (_, _, bob) = call(&app, Method::POST, "/auth/join", None, Some(json!({ "code": code, "display_name": "Bob", "username": "bob", "password": "geheim123" }))).await;
    let bob = bob.unwrap();

    let (st, body, _) = call(&app, Method::DELETE, &format!("/teams/{team_id}/membership"), Some(&coach), None).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{body}");
    // the viewer may leave; being on no team any more, the session is gone
    let (st, body, _) = call(&app, Method::DELETE, &format!("/teams/{team_id}/membership"), Some(&bob), None).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert!(body["active_team"].is_null());
    let (st, _, _) = call(&app, Method::GET, "/me", Some(&bob), None).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    // a second coach makes leaving possible
    let (_, _, cara) = call(&app, Method::POST, "/auth/join", None, Some(json!({ "code": me["team"]["join_code"], "display_name": "Cara", "username": "cara", "password": "geheim123" }))).await;
    let cara = cara.unwrap();
    let (_, body, _) = call(&app, Method::GET, "/me", Some(&cara), None).await;
    let cara_id = body["user"]["id"].as_i64().unwrap();
    let (st, _, _) = call(&app, Method::PATCH, &format!("/teams/{team_id}/members/{cara_id}"), Some(&coach), Some(json!({ "role": "coach" }))).await;
    assert_eq!(st, StatusCode::OK);
    let (st, body, _) = call(&app, Method::DELETE, &format!("/teams/{team_id}/membership"), Some(&coach), None).await;
    assert_eq!(st, StatusCode::OK, "{body}");
}

#[tokio::test]
async fn team_detail_counts_and_hides_the_code_from_non_coaches() {
    let app = app().await;
    let (coach, me) = register(&app, "Erste", "anna").await;
    let team_id = me["team"]["id"].as_i64().unwrap();
    for (n, p) in [(1, "Z"), (2, "A"), (3, "A"), (4, "M")] {
        call(&app, Method::POST, "/players", Some(&coach), Some(json!({ "number": n, "name": format!("P{n}"), "position": p }))).await;
    }
    let (_, pl, _) = call(&app, Method::GET, "/players", Some(&coach), None).await;
    let p4 = pl["players"].as_array().unwrap().iter().find(|p| p["number"] == 4).unwrap()["id"].as_i64().unwrap();
    call(&app, Method::PATCH, &format!("/players/{p4}"), Some(&coach), Some(json!({ "active": false }))).await;
    let (_, _, bob) = call(&app, Method::POST, "/auth/join", None, Some(json!({ "code": me["team"]["join_code"], "display_name": "Bob", "username": "bob", "password": "geheim123" }))).await;

    let (st, d, _) = call(&app, Method::GET, &format!("/teams/{team_id}"), Some(&coach), None).await;
    assert_eq!(st, StatusCode::OK, "{d}");
    assert_eq!(d["player_count"], 3);
    assert_eq!(d["inactive_count"], 1);
    assert_eq!(d["positions"]["A"], 2);
    assert_eq!(d["member_count"], 2);
    assert_eq!(d["role"], "coach");
    assert!(d["join_code"].is_string());
    let (st, d, _) = call(&app, Method::GET, &format!("/teams/{team_id}"), Some(&bob.unwrap()), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(d["role"], "viewer");
    assert!(d["join_code"].is_null());
    // not a member at all
    let (other, _) = register(&app, "Fremd", "dora").await;
    let (st, _, _) = call(&app, Method::GET, &format!("/teams/{team_id}"), Some(&other), None).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn per_team_stammdaten_need_a_coach_of_that_team() {
    let app = app().await;
    let (coach, me) = register(&app, "Erste", "anna").await;
    let team_id = me["team"]["id"].as_i64().unwrap();
    // anna founds a second team and is active there; editing the first one by id still works
    let (st, _, _) = call(&app, Method::POST, "/teams", Some(&coach), Some(json!({ "name": "Zweite" }))).await;
    assert_eq!(st, StatusCode::OK);
    let (st, body, _) = call(&app, Method::PATCH, &format!("/teams/{team_id}"), Some(&coach), Some(json!({ "league": "Landesliga" }))).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let (_, d, _) = call(&app, Method::GET, &format!("/teams/{team_id}"), Some(&coach), None).await;
    assert_eq!(d["league"], "Landesliga");
    // a viewer of that team may not
    let (_, _, bob) = call(&app, Method::POST, "/auth/join", None, Some(json!({ "code": me["team"]["join_code"], "display_name": "Bob", "username": "bob", "password": "geheim123" }))).await;
    let (st, _, _) = call(&app, Method::PATCH, &format!("/teams/{team_id}"), Some(&bob.unwrap()), Some(json!({ "league": "X" }))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
}
