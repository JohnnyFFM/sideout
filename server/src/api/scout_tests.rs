//! Scouting handover: ownership, protected writes, takeover, release and
//! expiry rules, driven through the router against a temp SQLite.

use axum::http::{Method, StatusCode};
use serde_json::{json, Value};

use super::tests::{app, call, call_h, register, App};

async fn login(app: &App, user: &str) -> String {
    let (st, body, cookie) = call(app, Method::POST, "/auth/login", None, Some(json!({ "username": user, "password": "geheim123" }))).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    cookie.unwrap()
}

/// a coach with a scoutable match (lineup set) and the join code
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

fn acquire(mode: &str, rev: i64, req: &str) -> Value {
    json!({ "mode": mode, "revision": rev, "request_id": req })
}

async fn claim(app: &App, cookie: &str, mid: i64, rev: i64, req: &str) -> (StatusCode, Value) {
    let (st, b, _) = call(app, Method::POST, &format!("/matches/{mid}/scout"), Some(cookie), Some(acquire("claim", rev, req))).await;
    (st, b)
}

async fn opp(app: &App, cookie: &str, mid: i64, seq: i64, cid: &str, lease: Option<&str>) -> (StatusCode, Value) {
    let (st, b, _) = call_h(app, Method::POST, &format!("/matches/{mid}/actions"), Some(cookie), Some(json!({ "seq": seq, "skill": "opp", "grade": "=", "cid": cid })), lease).await;
    (st, b)
}

async fn get(app: &App, cookie: &str, mid: i64) -> Value {
    let (st, m, _) = call(app, Method::GET, &format!("/matches/{mid}"), Some(cookie), None).await;
    assert_eq!(st, StatusCode::OK, "{m}");
    m
}

#[tokio::test]
async fn viewing_does_not_claim_and_unheld_state_is_explicit() {
    let app = app().await;
    let (c, mid, _) = fixture(&app).await;
    let m = get(&app, &c, mid).await;
    assert_eq!(m["scout"], json!({ "mine": false, "held": false, "stale": false, "actor": null, "device": null, "since": null, "last_seen_at": null, "revision": 0, "lease": null }));
    // list and home data carry the same block
    let (_, l, _) = call(&app, Method::GET, "/matches", Some(&c), None).await;
    assert_eq!(l["matches"][0]["scout"]["held"], false);
}

#[tokio::test]
async fn claims_race_and_protected_writes_need_the_lease() {
    let app = app().await;
    let (a1, mid, code) = fixture(&app).await;
    let a2 = login(&app, "anna").await; // same user, second device
    // both see the free snapshot at revision 0; the first claim wins
    let (st, m1) = claim(&app, &a1, mid, 0, "r1").await;
    assert_eq!(st, StatusCode::OK, "{m1}");
    let lease1 = m1["scout"]["lease"].as_str().unwrap().to_string();
    assert_eq!(m1["scout"]["mine"], true);
    assert_eq!(m1["scout"]["revision"], 1);
    let (st, e) = claim(&app, &a2, mid, 0, "r2").await;
    assert_eq!(st, StatusCode::CONFLICT);
    assert_eq!(e["error"], "scouted_elsewhere");
    assert_eq!(e["current"]["scout"]["held"], true);
    assert_eq!(e["current"]["scout"]["mine"], false);
    assert!(e["current"]["scout"]["lease"].is_null(), "the lease is never shown to another session");
    // a claim with a fresh snapshot still fails while the holder is current
    let (st, e) = claim(&app, &a2, mid, 1, "r3").await;
    assert_eq!(st, StatusCode::CONFLICT, "{e}");
    assert_eq!(e["error"], "scouted_elsewhere");

    // protected writes: no lease, foreign lease, wrong session
    let (st, e) = opp(&app, &a2, mid, 1, "x1", None).await;
    assert_eq!(st, StatusCode::CONFLICT);
    assert_eq!(e["error"], "scouted_elsewhere");
    let (st, e) = opp(&app, &a2, mid, 1, "x1", Some(&lease1)).await;
    assert_eq!(st, StatusCode::CONFLICT, "a lease is bound to its session");
    assert_eq!(e["error"], "scouted_elsewhere");
    let (st, r) = opp(&app, &a1, mid, 1, "x1", Some(&lease1)).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert_eq!(r["scout"]["mine"], true, "writes return the caller's scout state");
    // undo and lineup are protected too
    let (st, e, _) = call_h(&app, Method::DELETE, &format!("/matches/{mid}/actions/last?cid=x1"), Some(&a2), None, None).await;
    assert_eq!(st, StatusCode::CONFLICT, "{e}");
    let m = get(&app, &a1, mid).await;
    let pos = m["lineups"]["1"]["pos"].clone();
    let (st, e, _) = call_h(&app, Method::PUT, &format!("/matches/{mid}/lineups/2"), Some(&a2), Some(json!({ "pos": pos, "libero": null })), None).await;
    assert_eq!(st, StatusCode::CONFLICT, "{e}");
    assert_eq!(e["error"], "scouted_elsewhere");
    let (st, _, _) = call_h(&app, Method::PUT, &format!("/matches/{mid}/lineups/2"), Some(&a1), Some(json!({ "pos": pos, "libero": null })), Some(&lease1)).await;
    assert_eq!(st, StatusCode::OK);
    // first_serve is protected, notes are not
    let v = get(&app, &a1, mid).await["version"].as_i64().unwrap();
    let (st, e, _) = call_h(&app, Method::PATCH, &format!("/matches/{mid}"), Some(&a2), Some(json!({ "version": v, "first_serve": "them" })), None).await;
    assert_eq!(st, StatusCode::CONFLICT, "{e}");
    let (st, _, _) = call_h(&app, Method::PATCH, &format!("/matches/{mid}"), Some(&a2), Some(json!({ "version": v, "notes": "Halle B" })), None).await;
    assert_eq!(st, StatusCode::OK);

    // explicit takeover from the observed revision
    let rev = get(&app, &a2, mid).await["scout"]["revision"].as_i64().unwrap();
    let (st, m2, _) = call(&app, Method::POST, &format!("/matches/{mid}/scout"), Some(&a2), Some(acquire("takeover", rev, "t1"))).await;
    assert_eq!(st, StatusCode::OK, "{m2}");
    let lease2 = m2["scout"]["lease"].as_str().unwrap().to_string();
    assert_ne!(lease1, lease2);
    // the displaced device: its retry of an already committed action is refused, a new action too
    let (st, e) = opp(&app, &a1, mid, 1, "x1", Some(&lease1)).await;
    assert_eq!(st, StatusCode::CONFLICT);
    assert_eq!(e["error"], "scouted_elsewhere");
    let (st, _) = opp(&app, &a1, mid, 2, "x2", Some(&lease1)).await;
    assert_eq!(st, StatusCode::CONFLICT);
    let (st, _) = opp(&app, &a2, mid, 2, "x2", Some(&lease2)).await;
    assert_eq!(st, StatusCode::OK);
    // a takeover from a stale observation must not steal from the newer holder
    let (st, e, _) = call(&app, Method::POST, &format!("/matches/{mid}/scout"), Some(&a1), Some(acquire("takeover", rev, "t2"))).await;
    assert_eq!(st, StatusCode::CONFLICT, "{e}");
    assert_eq!(e["error"], "scouted_elsewhere");

    // viewers and other teams cannot acquire
    let (_, _, bob) = call(&app, Method::POST, "/auth/join", None, Some(json!({ "code": code, "display_name": "Bob", "username": "bob", "password": "geheim123" }))).await;
    let (st, _) = claim(&app, &bob.unwrap(), mid, 99, "v1").await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (other, _) = register(&app, "Fremd", "dora").await;
    let (st, _) = claim(&app, &other, mid, 99, "d1").await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn lost_answers_and_late_requests_are_harmless() {
    let app = app().await;
    let (a1, mid, _) = fixture(&app).await;
    let a2 = login(&app, "anna").await;
    // acquisition answer lost → the retry (same request id) returns the same lease, no new period
    let (_, m) = claim(&app, &a1, mid, 0, "r1").await;
    let lease1 = m["scout"]["lease"].as_str().unwrap().to_string();
    let (st, m) = claim(&app, &a1, mid, 0, "r1").await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(m["scout"]["lease"], lease1);
    assert_eq!(m["scout"]["revision"], 1);
    // a new claim by the holding session (another tab handing over) starts a
    // new period: new lease, revision advanced, the old lease is dead
    let (st, m) = claim(&app, &a1, mid, 1, "r1b").await;
    assert_eq!(st, StatusCode::OK);
    let lease1b = m["scout"]["lease"].as_str().unwrap().to_string();
    assert_ne!(lease1b, lease1);
    assert_eq!(m["scout"]["revision"], 2);
    let (st, _, _) = call(&app, Method::DELETE, &format!("/matches/{mid}/scout?lease={lease1}"), Some(&a1), None).await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    assert_eq!(get(&app, &a1, mid).await["scout"]["lease"], lease1b, "the late release of the old tab's lease changes nothing");
    let lease1 = lease1b;

    // takeover by device 2, then back by device 1; replaying device 2's old takeover must not steal
    let (st, m2, _) = call(&app, Method::POST, &format!("/matches/{mid}/scout"), Some(&a2), Some(acquire("takeover", 2, "t2"))).await;
    assert_eq!(st, StatusCode::OK, "{m2}");
    let (st, m3, _) = call(&app, Method::POST, &format!("/matches/{mid}/scout"), Some(&a1), Some(acquire("takeover", 3, "t3"))).await;
    assert_eq!(st, StatusCode::OK, "{m3}");
    let lease3 = m3["scout"]["lease"].as_str().unwrap().to_string();
    let (st, e, _) = call(&app, Method::POST, &format!("/matches/{mid}/scout"), Some(&a2), Some(acquire("takeover", 2, "t2"))).await;
    assert_eq!(st, StatusCode::CONFLICT, "{e}");
    assert_eq!(get(&app, &a1, mid).await["scout"]["lease"], lease3);

    // release: only the matching session + lease; repeated and late releases are no-ops
    let (st, _, _) = call(&app, Method::DELETE, &format!("/matches/{mid}/scout?lease={lease1}"), Some(&a1), None).await;
    assert_eq!(st, StatusCode::NO_CONTENT, "old lease: no-op");
    assert_eq!(get(&app, &a1, mid).await["scout"]["mine"], true);
    let (st, _, _) = call(&app, Method::DELETE, &format!("/matches/{mid}/scout?lease={lease3}"), Some(&a2), None).await;
    assert_eq!(st, StatusCode::NO_CONTENT, "other session: no-op");
    assert_eq!(get(&app, &a1, mid).await["scout"]["mine"], true);
    let (st, _, _) = call(&app, Method::DELETE, &format!("/matches/{mid}/scout?lease={lease3}"), Some(&a1), None).await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    let m = get(&app, &a1, mid).await;
    assert_eq!(m["scout"]["held"], false);
    assert_eq!(m["scout"]["revision"], 5);
    let (st, _, _) = call(&app, Method::DELETE, &format!("/matches/{mid}/scout?lease={lease3}"), Some(&a1), None).await;
    assert_eq!(st, StatusCode::NO_CONTENT, "repeated release");
    assert_eq!(get(&app, &a1, mid).await["scout"]["revision"], 5, "no transition without ownership change");
    // reacquire, then a delayed release of the previous lease of the same session changes nothing
    let (_, m) = claim(&app, &a1, mid, 5, "r5").await;
    let lease5 = m["scout"]["lease"].as_str().unwrap().to_string();
    let (st, _, _) = call(&app, Method::DELETE, &format!("/matches/{mid}/scout?lease={lease3}"), Some(&a1), None).await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    assert_eq!(get(&app, &a1, mid).await["scout"]["lease"], lease5);
    // and a write with the old lease of the same session is expired, not "elsewhere"
    let (st, e) = opp(&app, &a1, mid, 1, "y1", Some(&lease3)).await;
    assert_eq!(st, StatusCode::CONFLICT);
    assert_eq!(e["error"], "scout_lease_expired");
}

#[tokio::test]
async fn expiry_session_loss_and_role_loss_free_the_match() {
    let app = app().await;
    let (a1, mid, code) = fixture(&app).await;
    let a2 = login(&app, "anna").await;
    let (_, m) = claim(&app, &a1, mid, 0, "r1").await;
    let lease1 = m["scout"]["lease"].as_str().unwrap().to_string();
    // nine minutes idle: still current
    sqlx::query("UPDATE matches SET scout_seen = datetime('now', '-9 minutes') WHERE id = ?").bind(mid).execute(&app.state.dbw).await.unwrap();
    let (st, e) = claim(&app, &a2, mid, 1, "r2").await;
    assert_eq!(st, StatusCode::CONFLICT, "{e}");
    assert_eq!(get(&app, &a2, mid).await["scout"]["stale"], false);
    // eleven minutes: stale, visible as such, claimable; the old lease is dead
    sqlx::query("UPDATE matches SET scout_seen = datetime('now', '-11 minutes') WHERE id = ?").bind(mid).execute(&app.state.dbw).await.unwrap();
    let m = get(&app, &a2, mid).await;
    assert_eq!(m["scout"]["held"], true);
    assert_eq!(m["scout"]["stale"], true);
    let (st, m) = claim(&app, &a2, mid, 1, "r2").await;
    assert_eq!(st, StatusCode::OK, "{m}");
    let lease2 = m["scout"]["lease"].as_str().unwrap().to_string();
    let (st, e) = opp(&app, &a1, mid, 1, "z1", Some(&lease1)).await;
    assert_eq!(st, StatusCode::CONFLICT);
    assert_eq!(e["error"], "scouted_elsewhere");
    // an accepted write renews the holder
    let (st, _) = opp(&app, &a2, mid, 1, "z1", Some(&lease2)).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(get(&app, &a2, mid).await["scout"]["stale"], false);
    // a null last activity is treated as stale, defensively
    sqlx::query("UPDATE matches SET scout_seen = NULL WHERE id = ?").bind(mid).execute(&app.state.dbw).await.unwrap();
    assert_eq!(get(&app, &a1, mid).await["scout"]["stale"], true);
    let (st, m) = claim(&app, &a1, mid, 2, "r3").await;
    assert_eq!(st, StatusCode::OK, "{m}");
    let lease3 = m["scout"]["lease"].as_str().unwrap().to_string();

    // the holder logs out: its session row goes, the match is free for the other device
    let (st, _, _) = call(&app, Method::POST, "/auth/logout", Some(&a1), None).await;
    assert_eq!(st, StatusCode::OK);
    let m = get(&app, &a2, mid).await;
    assert_eq!(m["scout"]["held"], false, "{m}");
    let (st, m) = claim(&app, &a2, mid, m["scout"]["revision"].as_i64().unwrap(), "r4").await;
    assert_eq!(st, StatusCode::OK, "{m}");
    let lease4 = m["scout"]["lease"].as_str().unwrap().to_string();
    let _ = lease3;

    // role loss: a second coach demotes anna to viewer → her writes stop, the match is claimable
    let (_, _, cara) = call(&app, Method::POST, "/auth/join", None, Some(json!({ "code": code, "display_name": "Cara", "username": "cara", "password": "geheim123" }))).await;
    let cara = cara.unwrap();
    let (_, me, _) = call(&app, Method::GET, "/me", Some(&cara), None).await;
    let cara_id = me["user"]["id"].as_i64().unwrap();
    let anna_id = me["members"].as_array().unwrap().iter().find(|m| m["username"] == "anna").unwrap()["id"].as_i64().unwrap();
    let team_id = me["team"]["id"].as_i64().unwrap();
    let (st, _, _) = call(&app, Method::PATCH, &format!("/teams/{team_id}/members/{cara_id}"), Some(&a2), Some(json!({ "role": "coach" }))).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _, _) = call(&app, Method::PATCH, &format!("/teams/{team_id}/members/{anna_id}"), Some(&cara), Some(json!({ "role": "viewer" }))).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = opp(&app, &a2, mid, 2, "z2", Some(&lease4)).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "membership and role checks stay mandatory");
    let m = get(&app, &cara, mid).await;
    assert_eq!(m["scout"]["held"], false, "a holder without scouting rights does not block: {m}");
    let (st, m) = claim(&app, &cara, mid, m["scout"]["revision"].as_i64().unwrap(), "c1").await;
    assert_eq!(st, StatusCode::OK, "{m}");
}

#[tokio::test]
async fn takeover_and_in_flight_writes_never_interleave() {
    // the write pool has one connection and each protected write is one
    // transaction: whichever runs first wins; an append can only commit
    // under a lease that is current at commit time
    let app = app().await;
    let (a1, mid, _) = fixture(&app).await;
    let a2 = login(&app, "anna").await;
    let mut seq = 0;
    for round in 0..12 {
        let m = get(&app, &a1, mid).await;
        let rev = m["scout"]["revision"].as_i64().unwrap();
        let (st, m1, _) = call(&app, Method::POST, &format!("/matches/{mid}/scout"), Some(&a1), Some(acquire("takeover", rev, &format!("a{round}")))).await;
        assert_eq!(st, StatusCode::OK, "{m1}");
        let lease1 = m1["scout"]["lease"].as_str().unwrap().to_string();
        let rev1 = m1["scout"]["revision"].as_i64().unwrap();
        let cid = format!("c{round}");
        let append = opp(&app, &a1, mid, seq + 1, &cid, Some(&lease1));
        let path = format!("/matches/{mid}/scout");
        let take = call(&app, Method::POST, &path, Some(&a2), Some(acquire("takeover", rev1, &format!("b{round}"))));
        let ((ast, ab), (tst, tb, _)) = tokio::join!(append, take);
        assert_eq!(tst, StatusCode::OK, "{tb}");
        if ast == StatusCode::OK {
            seq += 1;
            assert_eq!(ab["scout"]["mine"], true, "an accepted append commits under its own current lease");
            assert_eq!(ab["scout"]["revision"], rev1, "…before the takeover advanced the revision");
        } else {
            assert_eq!(ast, StatusCode::CONFLICT, "{ab}");
            assert_eq!(ab["error"], "scouted_elsewhere");
        }
        let m = get(&app, &a1, mid).await;
        assert_eq!(m["actions"].as_array().unwrap().len() as i64, seq, "the log holds exactly the accepted appends");
        assert_eq!(m["scout"]["mine"], false, "device 2 holds the match after the round");
    }
}

#[tokio::test]
async fn finishing_and_reopening_through_leases() {
    let app = app().await;
    let (a1, mid, _) = fixture(&app).await;
    let (_, m) = claim(&app, &a1, mid, 0, "r1").await;
    let lease = m["scout"]["lease"].as_str().unwrap().to_string();
    // three sets of 25 opponent errors finish the match
    let mut seq = 0;
    for _ in 0..75 {
        seq += 1;
        let (st, b) = opp(&app, &a1, mid, seq, &format!("f{seq}"), Some(&lease)).await;
        assert_eq!(st, StatusCode::OK, "{b}");
    }
    let m = get(&app, &a1, mid).await;
    assert_eq!(m["status"], "done");
    assert_eq!(m["state"]["finished"], true);
    // the device releases after the final point; viewing does not reclaim
    let (st, _, _) = call(&app, Method::DELETE, &format!("/matches/{mid}/scout?lease={lease}"), Some(&a1), None).await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    let m = get(&app, &a1, mid).await;
    assert_eq!(m["scout"]["held"], false);
    // undo of the final point needs a new lease, then reopens the match
    let (st, e, _) = call_h(&app, Method::DELETE, &format!("/matches/{mid}/actions/last?cid=f75"), Some(&a1), None, Some(&lease)).await;
    assert_eq!(st, StatusCode::CONFLICT, "{e}");
    assert_eq!(e["error"], "scout_lease_expired");
    let (st, m) = claim(&app, &a1, mid, m["scout"]["revision"].as_i64().unwrap(), "r2").await;
    assert_eq!(st, StatusCode::OK, "{m}");
    let lease2 = m["scout"]["lease"].as_str().unwrap().to_string();
    let (st, r, _) = call_h(&app, Method::DELETE, &format!("/matches/{mid}/actions/last?cid=f75"), Some(&a1), None, Some(&lease2)).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert_eq!(get(&app, &a1, mid).await["status"], "live");
}
