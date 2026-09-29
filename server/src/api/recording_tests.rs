//! Recordings: upload contract (identity, retries, gaps, mismatch), the
//! coach's selection, team scoping, status, the legacy migration, archiving
//! and the 503 stubs of the old protocol — driven through the router.

use axum::http::{Method, StatusCode};
use serde_json::{json, Value};
use sqlx::Row;

use super::tests::{app, call, register, App};

/// a coach with a planned match (lineup set) and the join code
async fn fixture(app: &App) -> (String, i64, String) {
    let (c, me) = register(app, "Erste", "anna").await;
    let code = me["team"]["join_code"].as_str().unwrap().to_string();
    let mut ids = vec![];
    for n in 1..=7 {
        let (_, p, _) = call(app, Method::POST, "/players", Some(&c), Some(json!({ "number": n, "name": format!("P{n}"), "position": if n == 1 { "Z" } else if n == 7 { "L" } else { "A" } }))).await;
        ids.push(p["id"].as_i64().unwrap());
    }
    let (st, m, _) = call(app, Method::POST, "/matches", Some(&c), Some(json!({ "opponent": "Gegner", "date": "2026-09-21", "lineup": { "pos": &ids[..6], "libero": ids[6] } }))).await;
    assert_eq!(st, StatusCode::OK, "{m}");
    (c, m["id"].as_i64().unwrap(), code)
}

async fn get(app: &App, cookie: &str, mid: i64) -> Value {
    let (st, m, _) = call(app, Method::GET, &format!("/matches/{mid}"), Some(cookie), None).await;
    assert_eq!(st, StatusCode::OK, "{m}");
    m
}

/// the base a fresh recording of this match starts from: the planning and the roster
async fn base_for(app: &App, cookie: &str, mid: i64) -> Value {
    let m = get(app, cookie, mid).await;
    let roster: Vec<Value> = m["players"].as_array().unwrap().iter().map(|p| json!({ "id": p["id"], "number": p["number"], "name": p["name"], "position": p["position"] })).collect();
    json!({ "schema": 1, "first_serve_us": m["planning"]["first_serve"] == "us", "lineups": m["planning"]["lineups"], "roster": roster, "actions": [] })
}

fn opp(n: i64, seq: i64, grade: &str) -> Value {
    json!({ "n": n, "body": { "op": "add", "action": { "seq": seq, "skill": "opp", "grade": grade } } })
}
fn undo(n: i64, seq: i64) -> Value {
    json!({ "n": n, "body": { "op": "undo", "seq": seq } })
}

async fn upload(app: &App, cookie: &str, mid: i64, rid: &str, base: Option<Value>, edits: Vec<Value>) -> (StatusCode, Value) {
    let (st, b, _) = call(app, Method::PUT, &format!("/matches/{mid}/recordings/{rid}"), Some(cookie), Some(json!({ "device_id": "dev-a", "device_label": "iPhone", "base": base, "edits": edits }))).await;
    (st, b)
}

#[tokio::test]
async fn upload_creates_selects_confirms_and_shows_the_result() {
    let app = app().await;
    let (c, mid, _) = fixture(&app).await;
    let base = base_for(&app, &c, mid).await;
    let (st, b) = upload(&app, &c, mid, "r1", Some(base.clone()), vec![opp(1, 1, "="), opp(2, 2, "#"), undo(3, 2)]).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    assert_eq!(b["confirmed"], 3);
    assert_eq!(b["selected"], true);
    assert_eq!(b["status"], "live");
    assert_eq!(b["state"]["us"], 1);
    assert_eq!(b["state"]["them"], 0);
    let m = get(&app, &c, mid).await;
    assert_eq!(m["status"], "live");
    assert_eq!(m["selected"], "r1");
    assert_eq!(m["actions"].as_array().unwrap().len(), 1);
    assert_eq!(m["state"]["us"], 1);
    assert_eq!(m["recordings"][0]["id"], "r1");
    assert_eq!(m["recordings"][0]["n"], 3);
    assert_eq!(m["recordings"][0]["selected"], true);
    assert_eq!(m["recordings"][0]["device"], "iPhone");
    assert_eq!(m["recordings"][0]["user"], "ANNA");
    // the list carries the state and who is scouting right now
    let (_, l, _) = call(&app, Method::GET, "/matches", Some(&c), None).await;
    assert_eq!(l["matches"][0]["state"]["us"], 1);
    assert_eq!(l["matches"][0]["scouting"]["device"], "iPhone");
    assert_eq!(l["matches"][0]["recordings"], 1);
    // the recording itself: base, edits, folded snapshot
    let (st, r, _) = call(&app, Method::GET, &format!("/matches/{mid}/recordings/r1"), Some(&c), None).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert_eq!(r["edits"].as_array().unwrap().len(), 3);
    assert_eq!(r["edits"][2]["body"]["op"], "undo");
    assert_eq!(r["snapshot"]["actions"].as_array().unwrap().len(), 1);
    assert_eq!(r["base"]["roster"].as_array().unwrap().len(), 7);
}

#[tokio::test]
async fn retries_are_idempotent_and_different_content_is_refused() {
    let app = app().await;
    let (c, mid, _) = fixture(&app).await;
    let base = base_for(&app, &c, mid).await;
    let (st, _) = upload(&app, &c, mid, "r1", Some(base.clone()), vec![opp(1, 1, "="), opp(2, 2, "=")]).await;
    assert_eq!(st, StatusCode::OK);
    // same edits again, other key order and whitespace irrelevant, no base: fine
    let same: Value = serde_json::from_str(r#"{ "n": 2, "body": { "action": { "grade": "=", "skill": "opp", "seq": 2, "player_id": null }, "op": "add" } }"#).unwrap();
    let (st, b) = upload(&app, &c, mid, "r1", None, vec![opp(1, 1, "="), same, opp(3, 3, "#")]).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    assert_eq!(b["confirmed"], 3);
    // the base again: identical is fine, different is refused
    let (st, _) = upload(&app, &c, mid, "r1", Some(base.clone()), vec![]).await;
    assert_eq!(st, StatusCode::OK);
    let mut other = base.clone();
    other["first_serve_us"] = json!(false);
    let (st, b) = upload(&app, &c, mid, "r1", Some(other), vec![]).await;
    assert_eq!(st, StatusCode::CONFLICT);
    assert_eq!(b["error"], "recording_mismatch");
    // a different edit under a stored number
    let (st, b) = upload(&app, &c, mid, "r1", None, vec![opp(2, 2, "#")]).await;
    assert_eq!(st, StatusCode::CONFLICT);
    assert_eq!(b["error"], "edit_mismatch");
    assert_eq!(b["current"]["n"], 2);
    // nothing of that was stored
    let m = get(&app, &c, mid).await;
    assert_eq!(m["recordings"][0]["n"], 3);
    assert_eq!(m["state"]["us"], 2);
    assert_eq!(m["state"]["them"], 1);
    // a recording id cannot be reused on another match
    let (st2, m2, _) = call(&app, Method::POST, "/matches", Some(&c), Some(json!({ "opponent": "Zwei", "date": "2026-09-22" }))).await;
    assert_eq!(st2, StatusCode::OK);
    let (st, b) = upload(&app, &c, m2["id"].as_i64().unwrap(), "r1", Some(base.clone()), vec![]).await;
    assert_eq!(st, StatusCode::CONFLICT, "{b}");
    assert_eq!(b["error"], "recording_mismatch");
}

#[tokio::test]
async fn a_gap_is_answered_with_the_confirmed_number_and_nothing_beyond_is_stored() {
    let app = app().await;
    let (c, mid, _) = fixture(&app).await;
    let base = base_for(&app, &c, mid).await;
    let (st, b) = upload(&app, &c, mid, "r1", Some(base), vec![opp(1, 1, "="), opp(3, 3, "=")]).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    assert_eq!(b["confirmed"], 1);
    let (st, b) = upload(&app, &c, mid, "r1", None, vec![opp(2, 2, "="), opp(3, 3, "=")]).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    assert_eq!(b["confirmed"], 3);
    // invalid content is refused as such, and stores nothing
    let (st, b) = upload(&app, &c, mid, "r1", None, vec![json!({ "n": 4, "body": { "op": "add", "action": { "seq": 4, "skill": "A", "grade": "#" } } })]).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{b}");
    let (st, b) = upload(&app, &c, mid, "r1", None, vec![json!({ "n": 4, "body": { "op": "undo", "seq": 3, "set": 1 } })]).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{b}");
    let m = get(&app, &c, mid).await;
    assert_eq!(m["recordings"][0]["n"], 3);
    // a missing base on a new recording
    let (st, _) = upload(&app, &c, mid, "r9", None, vec![opp(1, 1, "=")]).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn second_recording_keeps_the_selection_until_the_coach_picks() {
    let app = app().await;
    let (c, mid, code) = fixture(&app).await;
    let base = base_for(&app, &c, mid).await;
    let (st, _) = upload(&app, &c, mid, "r1", Some(base.clone()), vec![opp(1, 1, "=")]).await;
    assert_eq!(st, StatusCode::OK);
    // an assistant joins and records the same match on her own device
    let (st, body, bob) = call(&app, Method::POST, "/auth/join", None, Some(json!({ "code": code, "display_name": "Bob", "username": "bob", "password": "geheim123" }))).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let bob = bob.unwrap();
    let team_id = body["team"]["id"].as_i64().unwrap();
    let bob_id = body["user"]["id"].as_i64().unwrap();
    let (st, _) = upload(&app, &bob, mid, "r2", Some(base.clone()), vec![opp(1, 1, "#"), opp(2, 2, "#")]).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "a viewer cannot upload");
    call(&app, Method::PATCH, &format!("/teams/{team_id}/members/{bob_id}"), Some(&c), Some(json!({ "role": "assistant" }))).await;
    let (st, b) = upload(&app, &bob, mid, "r2", Some(base.clone()), vec![opp(1, 1, "#"), opp(2, 2, "#")]).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    assert_eq!(b["selected"], false);
    let m = get(&app, &c, mid).await;
    assert_eq!(m["selected"], "r1");
    assert_eq!(m["state"]["us"], 1);
    assert_eq!(m["recordings"].as_array().unwrap().len(), 2);
    assert_eq!(m["recordings"][1]["state"]["them"], 2);
    let rev = m["selection_rev"].as_i64().unwrap();
    // the assistant may not select, the coach may; a stale revision is refused
    let (st, _, _) = call(&app, Method::POST, &format!("/matches/{mid}/select"), Some(&bob), Some(json!({ "recording_id": "r2", "rev": rev }))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, b, _) = call(&app, Method::POST, &format!("/matches/{mid}/select"), Some(&c), Some(json!({ "recording_id": "r2", "rev": rev - 1 }))).await;
    assert_eq!(st, StatusCode::CONFLICT, "{b}");
    assert_eq!(b["error"], "selection_moved");
    let (st, b, _) = call(&app, Method::POST, &format!("/matches/{mid}/select"), Some(&c), Some(json!({ "recording_id": "r2", "rev": rev }))).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    assert_eq!(b["selected"], "r2");
    assert_eq!(b["state"]["them"], 2);
    assert_eq!(b["actions"].as_array().unwrap().len(), 2);
    // the selection follows further edits of r2, not of r1
    let (st, _) = upload(&app, &bob, mid, "r2", None, vec![opp(3, 3, "#")]).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = upload(&app, &c, mid, "r1", None, vec![opp(2, 2, "="), opp(3, 3, "=")]).await;
    assert_eq!(st, StatusCode::OK);
    let m = get(&app, &c, mid).await;
    assert_eq!(m["state"]["them"], 3);
    assert_eq!(m["state"]["us"], 0);
    let (st, _, _) = call(&app, Method::POST, &format!("/matches/{mid}/select"), Some(&c), Some(json!({ "recording_id": "nope", "rev": rev + 1 }))).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_import_never_counts_as_live_scouting() {
    let app = app().await;
    let (c, mid, _) = fixture(&app).await;
    let base = base_for(&app, &c, mid).await;
    let (st, b, _) = call(&app, Method::PUT, &format!("/matches/{mid}/recordings/imp1"), Some(&c), Some(json!({ "imported": true, "device_id": "legacy", "device_label": "Import (alt)", "base": base, "edits": [opp(1, 1, "=")] }))).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    let (_, l, _) = call(&app, Method::GET, "/matches", Some(&c), None).await;
    assert!(l["matches"][0]["scouting"].is_null(), "an import is not someone scouting right now");
    let m = get(&app, &c, mid).await;
    assert_eq!(m["recordings"][0]["imported"], true);
    assert_eq!(m["recordings"][0]["active"], false);
    assert_eq!(m["state"]["us"], 1, "but it is a recording like any other");
}

#[tokio::test]
async fn an_upload_names_its_account_and_a_foreign_cookie_is_refused() {
    let app = app().await;
    let (c, mid, _) = fixture(&app).await;
    let (_, me, _) = call(&app, Method::GET, "/me", Some(&c), None).await;
    let uid = me["user"]["id"].as_i64().unwrap();
    let base = base_for(&app, &c, mid).await;
    let body = |uploader: i64| json!({ "uploader": uploader, "device_id": "d", "device_label": "x", "base": base, "edits": [opp(1, 1, "=")] });
    let (st, b, _) = call(&app, Method::PUT, &format!("/matches/{mid}/recordings/r1"), Some(&c), Some(body(uid + 7))).await;
    assert_eq!(st, StatusCode::CONFLICT, "{b}");
    assert_eq!(b["error"], "account_mismatch");
    assert_eq!(b["current"]["user_id"], uid);
    let (_, m, _) = call(&app, Method::GET, &format!("/matches/{mid}"), Some(&c), None).await;
    assert!(m["recordings"].as_array().unwrap().is_empty(), "nothing stored under the wrong account");
    let (st, b, _) = call(&app, Method::PUT, &format!("/matches/{mid}/recordings/r1"), Some(&c), Some(body(uid))).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    assert_eq!(b["confirmed"], 1);
}

#[tokio::test]
async fn upload_resolves_the_team_from_the_match_not_the_session() {
    let app = app().await;
    let (c, mid, _) = fixture(&app).await;
    let base = base_for(&app, &c, mid).await;
    // the coach founds a second team and switches her session there
    let (st, t2, _) = call(&app, Method::POST, "/teams", Some(&c), Some(json!({ "name": "Zweite" }))).await;
    assert_eq!(st, StatusCode::OK, "{t2}");
    let (_, me, _) = call(&app, Method::GET, "/me", Some(&c), None).await;
    assert_eq!(me["team"]["name"], "Zweite");
    // the first team's match is not visible from here, but the upload lands
    let (st, _, _) = call(&app, Method::GET, &format!("/matches/{mid}"), Some(&c), None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, b) = upload(&app, &c, mid, "r1", Some(base.clone()), vec![opp(1, 1, "=")]).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    let (st, r, _) = call(&app, Method::GET, &format!("/matches/{mid}/recordings/r1"), Some(&c), None).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    // a stranger gets nothing
    let (other, _) = register(&app, "Fremd", "dora").await;
    let (st, _) = upload(&app, &other, mid, "r7", Some(base), vec![opp(1, 1, "=")]).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    // the audit line went to the match's team
    let team1: i64 = sqlx::query("SELECT team_id FROM matches WHERE id = ?").bind(mid).fetch_one(&app.state.db).await.unwrap().get("team_id");
    let n: i64 = sqlx::query("SELECT count(*) AS n FROM audit_log WHERE team_id = ? AND action = 'recording'").bind(team1).fetch_one(&app.state.db).await.unwrap().get("n");
    assert_eq!(n, 1);
}

#[tokio::test]
async fn a_finished_recording_sets_done_and_an_undo_reopens() {
    let app = app().await;
    let (c, mid, _) = fixture(&app).await;
    let base = base_for(&app, &c, mid).await;
    // 25 opponent errors win a set; three sets finish the match
    let edits: Vec<Value> = (1..=75).map(|i| opp(i, i, "=")).collect();
    let (st, b) = upload(&app, &c, mid, "r1", Some(base), edits).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    assert_eq!(b["status"], "done");
    assert_eq!(b["state"]["finished"], true);
    let (_, l, _) = call(&app, Method::GET, "/matches", Some(&c), None).await;
    assert_eq!(l["matches"][0]["status"], "done");
    let (_, s, _) = call(&app, Method::GET, "/season/stats", Some(&c), None).await;
    assert_eq!(s["team"]["won"], 1);
    let (st, b) = upload(&app, &c, mid, "r1", None, vec![undo(76, 75)]).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    assert_eq!(b["status"], "live");
    let (st, csv, _) = call(&app, Method::GET, &format!("/matches/{mid}/export.csv"), Some(&c), None).await;
    assert_eq!(st, StatusCode::OK);
    let _ = csv;
    let (st, stats, _) = call(&app, Method::GET, &format!("/matches/{mid}/stats"), Some(&c), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(stats["team"]["ptsBy"]["opp"], 74);
}

#[tokio::test]
async fn unknown_players_in_a_recording_are_kept_and_named() {
    let app = app().await;
    let (c, mid, _) = fixture(&app).await;
    let mut base = base_for(&app, &c, mid).await;
    // a player the team never had: carried by the snapshot roster
    base["roster"].as_array_mut().unwrap().push(json!({ "id": 999, "number": 42, "name": "Gast", "position": "A" }));
    let att = json!({ "n": 1, "body": { "op": "add", "action": { "seq": 1, "skill": "A", "grade": "#", "player_id": 999 } } });
    let att2 = json!({ "n": 2, "body": { "op": "add", "action": { "seq": 2, "skill": "A", "grade": "#", "player_id": 998 } } });
    let (st, b) = upload(&app, &c, mid, "r1", Some(base), vec![att, att2]).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    let (_, stats, _) = call(&app, Method::GET, &format!("/matches/{mid}/stats"), Some(&c), None).await;
    let players = stats["players"].as_array().unwrap();
    let gast = players.iter().find(|p| p["id"] == 999).expect("snapshot player in stats");
    assert_eq!(gast["name"], "Gast");
    assert_eq!(gast["pts"], 1);
    let unknown = players.iter().find(|p| p["id"] == 998).expect("unknown id still counted");
    assert_eq!(unknown["name"], "Unbekannt (ID 998)");
    assert_eq!(unknown["pts"], 1);
    // removing a player from the roster deactivates, never deletes
    let m = get(&app, &c, mid).await;
    let pid = m["players"][0]["id"].as_i64().unwrap();
    let (st, r, _) = call(&app, Method::DELETE, &format!("/players/{pid}"), Some(&c), None).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert_eq!(r["deactivated"], true);
    let (_, pl, _) = call(&app, Method::GET, "/players", Some(&c), None).await;
    assert!(pl["players"].as_array().unwrap().iter().any(|p| p["id"] == pid && p["active"] == false));
}

#[tokio::test]
async fn the_legacy_log_becomes_a_selected_recording() {
    let app = app().await;
    let (c, mid, _) = fixture(&app).await;
    let m = get(&app, &c, mid).await;
    let p1 = m["players"][0]["id"].as_i64().unwrap();
    for (seq, skill, grade, pid) in [(1, "S", "#", Some(p1)), (2, "opp", "#", None), (3, "opp", "=", None)] {
        sqlx::query("INSERT INTO legacy_actions (match_id, seq, set_no, skill, grade, player_id) VALUES (?, ?, 1, ?, ?, ?)")
            .bind(mid).bind(seq).bind(skill).bind(grade).bind(pid)
            .execute(&app.state.dbw).await.unwrap();
    }
    let n = crate::store::migrate_legacy(&app.state.dbw).await.unwrap();
    assert_eq!(n, 1);
    let m = get(&app, &c, mid).await;
    assert_eq!(m["selected"], format!("legacy-{mid}"));
    assert_eq!(m["actions"].as_array().unwrap().len(), 3);
    assert_eq!(m["state"]["us"], 2);
    assert_eq!(m["recordings"][0]["device"], "Import");
    let left: i64 = sqlx::query("SELECT count(*) AS n FROM legacy_actions").fetch_one(&app.state.db).await.unwrap().get("n");
    assert_eq!(left, 0);
    // a second run finds nothing and changes nothing
    assert_eq!(crate::store::migrate_legacy(&app.state.dbw).await.unwrap(), 0);
    // the status column follows on the next write; here the import left it planned, a refresh sets live
    let (st, b) = upload(&app, &c, mid, &format!("legacy-{mid}"), None, vec![opp(1, 4, "=")]).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    assert_eq!(b["status"], "live");
    assert_eq!(b["state"]["us"], 3);
}

#[tokio::test]
async fn every_match_is_archived_not_deleted_and_old_routes_answer_503() {
    let app = app().await;
    let (c, mid, _) = fixture(&app).await;
    // no recording on the server yet: still archived, a phone may hold one offline
    let (st, m2, _) = call(&app, Method::POST, "/matches", Some(&c), Some(json!({ "opponent": "Leer", "date": "2026-09-22", "lineup": null }))).await;
    assert_eq!(st, StatusCode::OK);
    let m2id = m2["id"].as_i64().unwrap();
    let (st, r, _) = call(&app, Method::DELETE, &format!("/matches/{m2id}"), Some(&c), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(r["archived"], true);
    let base2 = base_for(&app, &c, mid).await;
    let (st, b) = upload(&app, &c, m2id, "r-late", Some(base2), vec![opp(1, 1, "=")]).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    let base = base_for(&app, &c, mid).await;
    let (st, _) = upload(&app, &c, mid, "r1", Some(base), vec![opp(1, 1, "=")]).await;
    assert_eq!(st, StatusCode::OK);
    let (st, r, _) = call(&app, Method::DELETE, &format!("/matches/{mid}"), Some(&c), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(r["archived"], true);
    let (_, l, _) = call(&app, Method::GET, "/matches", Some(&c), None).await;
    assert!(l["matches"].as_array().unwrap().is_empty());
    // a late upload still lands
    let (st, b) = upload(&app, &c, mid, "r1", None, vec![opp(2, 2, "=")]).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    assert_eq!(b["confirmed"], 2);
    // the old client's write routes
    let (st, b, _) = call(&app, Method::POST, &format!("/matches/{mid}/actions"), Some(&c), Some(json!({ "seq": 3, "skill": "opp", "grade": "=" }))).await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE, "{b}");
    let (st, _, _) = call(&app, Method::DELETE, &format!("/matches/{mid}/actions/last?seq=2"), Some(&c), None).await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE);
    let (st, _, _) = call(&app, Method::POST, &format!("/matches/{mid}/scout"), Some(&c), Some(json!({ "mode": "claim", "revision": 0, "request_id": "x" }))).await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE);
    let (st, _, _) = call(&app, Method::DELETE, &format!("/matches/{mid}/scout?lease=x"), Some(&c), None).await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn planning_edits_do_not_touch_a_started_recording() {
    let app = app().await;
    let (c, mid, _) = fixture(&app).await;
    let base = base_for(&app, &c, mid).await;
    let (st, _) = upload(&app, &c, mid, "r1", Some(base), vec![opp(1, 1, "=")]).await;
    assert_eq!(st, StatusCode::OK);
    let m = get(&app, &c, mid).await;
    let v = m["version"].as_i64().unwrap();
    let (st, b, _) = call(&app, Method::PATCH, &format!("/matches/{mid}"), Some(&c), Some(json!({ "version": v, "first_serve": "them", "opponent": "Neu" }))).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    assert_eq!(b["opponent"], "Neu");
    assert_eq!(b["planning"]["first_serve"], "them");
    assert_eq!(b["first_serve"], "us", "the result keeps the recording's first serve");
    // the recording changes its own first serve through an edit
    let (st, _) = upload(&app, &c, mid, "r1", None, vec![json!({ "n": 2, "body": { "op": "first_serve", "us": false } })]).await;
    assert_eq!(st, StatusCode::OK);
    let m = get(&app, &c, mid).await;
    assert_eq!(m["first_serve"], "them");
    // the status is derived, a stale version is a conflict
    let (st, _, _) = call(&app, Method::PATCH, &format!("/matches/{mid}"), Some(&c), Some(json!({ "version": v, "opponent": "Alt" }))).await;
    assert_eq!(st, StatusCode::CONFLICT);
}
