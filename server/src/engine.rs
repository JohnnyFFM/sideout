//! The match engine: replay an action log into score / rotation / serve /
//! set boundaries, and project it into Data-Volley-style statistics.
//! Pure functions, no I/O. `web/src/lib/engine.js` is the same algorithm
//! for optimistic + offline rendering; `tests/fixtures` pins them together.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const GRADES: [&str; 6] = ["#", "+", "!", "-", "/", "="];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lineup {
    pub pos: [i64; 6],
    pub libero: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MatchConfig {
    pub first_serve_us: bool,
    /// set_no → lineup; a set without an entry inherits the closest lower one
    pub lineups: BTreeMap<i64, Lineup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    pub id: i64,
    pub seq: i64,
    pub skill: String,
    pub grade: Option<String>,
    pub player_id: Option<i64>,
    pub sub_out: Option<i64>,
    pub sub_in: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Player {
    pub id: i64,
    pub number: i64,
    pub name: String,
    pub position: String,
}

/// 'us' | 'them' | None (rally continues)
pub fn outcome(skill: &str, grade: Option<&str>) -> Option<&'static str> {
    let g = grade.unwrap_or("");
    match skill {
        "opp" => Some(if g == "=" { "us" } else { "them" }),   // opponent error → our point
        "adj" => Some(if g == "#" { "us" } else { "them" }),   // catch-up: # us, = them
        "sub" | "lib" | "rot" | "srv" => None,
        _ => {
            if g == "=" {
                Some("them")
            } else if g == "/" && skill == "A" {
                Some("them")
            } else if g == "#" && (skill == "A" || skill == "S" || skill == "B") {
                Some("us")
            } else {
                None
            }
        }
    }
}

pub fn set_target(set: i64) -> i64 {
    if set == 5 { 15 } else { 25 }
}

#[derive(Debug, Clone, Serialize)]
pub struct Row {
    pub id: i64,
    pub seq: i64,
    pub set: i64,
    pub rally: i64,
    pub us: i64,
    pub them: i64,
    pub serving: bool,
    pub rot: i64,
    pub skill: String,
    pub grade: Option<String>,
    pub player_id: Option<i64>,
    pub sub_out: Option<i64>,
    pub sub_in: Option<i64>,
    pub outcome: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Rally {
    pub set: i64,
    pub rally: i64,
    pub serving: bool,
    pub won: bool,
    pub rot: i64,
    pub us: i64,
    pub them: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SetScore {
    pub us: i64,
    pub them: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct State {
    pub set: i64,
    pub us: i64,
    pub them: i64,
    pub sets: Vec<SetScore>,
    pub sets_won: i64,
    pub sets_lost: i64,
    pub lineup: [i64; 6],
    pub libero: Option<i64>,
    /// explicit libero assignment ('lib' action): the player she stands in
    /// for; None = automatic (the back-row middle)
    pub libero_for: Option<i64>,
    /// 'lib' action without a libero (sub_in NULL): she sits until dragged in again
    pub libero_off: bool,
    pub serving: bool,
    pub rally: i64,
    pub finished: bool,
    pub last_seq: i64,
    #[serde(skip)]
    pub rows: Vec<Row>,
    #[serde(skip)]
    pub rally_log: Vec<Rally>,
}

fn lineup_for(cfg: &MatchConfig, set: i64) -> Lineup {
    cfg.lineups
        .range(..=set)
        .next_back()
        .map(|(_, l)| l.clone())
        .or_else(|| cfg.lineups.values().next().cloned())
        .unwrap_or(Lineup { pos: [0; 6], libero: None })
}

fn rotate(l: [i64; 6]) -> [i64; 6] {
    [l[1], l[2], l[3], l[4], l[5], l[0]]
}

pub fn replay(cfg: &MatchConfig, actions: &[Action]) -> State {
    let first = lineup_for(cfg, 1);
    let mut st = State {
        set: 1,
        us: 0,
        them: 0,
        sets: vec![],
        sets_won: 0,
        sets_lost: 0,
        lineup: first.pos,
        libero: first.libero,
        libero_for: None,
        libero_off: false,
        serving: cfg.first_serve_us,
        rally: 1,
        finished: false,
        last_seq: 0,
        rows: vec![],
        rally_log: vec![],
    };
    let mut set_serve_start = cfg.first_serve_us;
    for a in actions {
        if st.finished {
            break;
        }
        st.last_seq = a.seq;
        if a.skill == "rot" || a.skill == "srv" {
            if a.skill == "rot" { st.lineup = rotate(st.lineup); } else { st.serving = a.grade.as_deref() == Some("#"); }
            st.rows.push(Row {
                id: a.id, seq: a.seq, set: st.set, rally: st.rally, us: st.us, them: st.them,
                serving: st.serving, rot: st.lineup[0], skill: a.skill.clone(), grade: a.grade.clone(),
                player_id: None, sub_out: None, sub_in: None, outcome: None,
            });
            continue;
        }
        if a.skill == "sub" || a.skill == "lib" {
            if a.skill == "lib" {
                if a.sub_in.is_none() {
                    st.libero_off = true;
                    st.libero_for = None;
                } else {
                    st.libero_off = false;
                    st.libero_for = a.sub_out;
                }
            } else if let (Some(out), Some(inn)) = (a.sub_out, a.sub_in) {
                if let Some(k) = st.lineup.iter().position(|&p| p == out) {
                    st.lineup[k] = inn;
                }
                if st.libero_for == Some(out) {
                    st.libero_for = None;
                }
            }
            st.rows.push(Row {
                id: a.id, seq: a.seq, set: st.set, rally: st.rally, us: st.us, them: st.them,
                serving: st.serving, rot: st.lineup[0], skill: a.skill.clone(), grade: None,
                player_id: None, sub_out: a.sub_out, sub_in: a.sub_in, outcome: None,
            });
            continue;
        }
        let out = outcome(&a.skill, a.grade.as_deref());
        st.rows.push(Row {
            id: a.id, seq: a.seq, set: st.set, rally: st.rally, us: st.us, them: st.them,
            serving: st.serving, rot: st.lineup[0], skill: a.skill.clone(), grade: a.grade.clone(),
            player_id: a.player_id, sub_out: None, sub_in: None, outcome: out,
        });
        let Some(out) = out else { continue };
        let won = out == "us";
        if a.skill == "adj" {
            // catch-up point: score only, no rally, rotation or serve change
            if won { st.us += 1 } else { st.them += 1 }
        } else {
            st.rally_log.push(Rally {
                set: st.set, rally: st.rally, serving: st.serving, won, rot: st.lineup[0], us: st.us, them: st.them,
            });
            let won_on_receive = won && !st.serving;
            if won { st.us += 1 } else { st.them += 1 }
            if won_on_receive {
                st.lineup = rotate(st.lineup);
            }
            st.serving = won;
            st.rally += 1;
        }
        let tgt = set_target(st.set);
        if (st.us >= tgt || st.them >= tgt) && (st.us - st.them).abs() >= 2 {
            st.sets.push(SetScore { us: st.us, them: st.them });
            let w = st.sets.iter().filter(|s| s.us > s.them).count();
            let l = st.sets.len() - w;
            if w == 3 || l == 3 {
                st.finished = true;
                break;
            }
            st.set += 1;
            st.rally = 1;
            set_serve_start = !set_serve_start;
            let l = lineup_for(cfg, st.set);
            st.lineup = l.pos;
            st.libero = l.libero;
            st.serving = set_serve_start;
            st.us = 0;
            st.them = 0;
        }
    }
    st.sets_won = st.sets.iter().filter(|s| s.us > s.them).count() as i64;
    st.sets_lost = st.sets.len() as i64 - st.sets_won;
    st
}

// ---------------------------------------------------------------- stats

fn reception_score(g: &str) -> i64 {
    match g { "#" => 3, "+" => 2, "!" => 1, _ => 0 }
}

#[derive(Default)]
struct PStat {
    pts: i64, n: i64,
    a_n: i64, a_k: i64, a_e: i64, a_blk: i64,
    s_n: i64, s_ace: i64, s_err: i64, s_pos: i64,
    r_n: i64, r_perf: i64, r_pos: i64, r_err: i64, r_sum: i64,
    b_pts: i64, b_touch: i64, b_err: i64,
    d_n: i64, d_good: i64, d_err: i64,
    e_n: i64, e_ast: i64, e_err: i64,
    grades: BTreeMap<String, BTreeMap<String, i64>>,
}

fn ratio(a: i64, n: i64) -> Value {
    if n == 0 { Value::Null } else { json!(a as f64 / n as f64) }
}

/// Stats for the whole match (`set` = None) or one set. Shape mirrors the
/// JS engine so the web app renders both identically.
pub fn stats(cfg: &MatchConfig, players: &[Player], actions: &[Action], set: Option<i64>) -> Value {
    let rp = replay(cfg, actions);
    let rows: Vec<&Row> = rp.rows.iter().filter(|r| set.map_or(true, |s| r.set == s)).collect();
    let rallies: Vec<&Rally> = rp.rally_log.iter().filter(|r| set.map_or(true, |s| r.set == s)).collect();

    let mut p: BTreeMap<i64, PStat> = BTreeMap::new();
    let mut pts_by = json!({"A": 0, "S": 0, "B": 0, "opp": 0});
    let mut err_by = json!({"S": 0, "R": 0, "E": 0, "A": 0, "B": 0, "D": 0});
    let mut lost_opp_kill = 0i64;
    let mut lost_err = 0i64;
    let mut team_grades: BTreeMap<String, BTreeMap<String, i64>> = BTreeMap::new();
    let inc = |v: &mut Value, k: &str| { v[k] = json!(v[k].as_i64().unwrap_or(0) + 1); };

    let mut last_set: Option<i64> = None; // setter id of the set preceding an attack
    for r in &rows {
        if matches!(r.skill.as_str(), "sub" | "lib" | "opp" | "adj" | "rot" | "srv") {
            if r.skill == "opp" {
                if r.grade.as_deref() == Some("=") { inc(&mut pts_by, "opp") } else { lost_opp_kill += 1 }
            }
            last_set = None;
            continue;
        }
        let Some(pid) = r.player_id else { continue };
        let g = r.grade.clone().unwrap_or_default();
        let ps = p.entry(pid).or_default();
        ps.n += 1;
        *ps.grades.entry(r.skill.clone()).or_default().entry(g.clone()).or_default() += 1;
        *team_grades.entry(r.skill.clone()).or_default().entry(g.clone()).or_default() += 1;
        if g == "=" { inc(&mut err_by, &r.skill); lost_err += 1; }
        match r.skill.as_str() {
            "A" => {
                ps.a_n += 1;
                match g.as_str() {
                    "#" => {
                        ps.a_k += 1; ps.pts += 1; inc(&mut pts_by, "A");
                        if let Some(sid) = last_set { if let Some(s) = p.get_mut(&sid) { s.e_ast += 1; } }
                    }
                    "=" => { p.get_mut(&pid).unwrap().a_e += 1; }
                    "/" => { p.get_mut(&pid).unwrap().a_blk += 1; lost_err += 1; }
                    _ => {}
                }
            }
            "S" => {
                ps.s_n += 1;
                match g.as_str() {
                    "#" => { ps.s_ace += 1; ps.pts += 1; inc(&mut pts_by, "S"); }
                    "=" => ps.s_err += 1,
                    "+" => ps.s_pos += 1,
                    _ => {}
                }
            }
            "R" => {
                ps.r_n += 1; ps.r_sum += reception_score(&g);
                match g.as_str() {
                    "#" => { ps.r_perf += 1; ps.r_pos += 1; }
                    "+" => ps.r_pos += 1,
                    "=" => ps.r_err += 1,
                    _ => {}
                }
            }
            "B" => match g.as_str() {
                "#" => { ps.b_pts += 1; ps.pts += 1; inc(&mut pts_by, "B"); }
                "=" => ps.b_err += 1,
                _ => ps.b_touch += 1,
            },
            "D" => {
                ps.d_n += 1;
                if g == "=" { ps.d_err += 1 } else if g != "-" && g != "/" { ps.d_good += 1 }
            }
            "E" => { ps.e_n += 1; if g == "=" { ps.e_err += 1 } }
            _ => {}
        }
        last_set = match r.skill.as_str() { "E" => Some(pid), "A" => None, _ => last_set };
    }

    let mut so = (0i64, 0i64);
    let mut brk = (0i64, 0i64);
    let mut us = 0i64;
    let mut them = 0i64;
    let mut by_rot: BTreeMap<i64, (i64, i64, i64, i64)> = BTreeMap::new(); // so_won, so_n, brk_won, brk_n
    for r in &rallies {
        let t = if r.serving { &mut brk } else { &mut so };
        t.1 += 1; if r.won { t.0 += 1 }
        if r.won { us += 1 } else { them += 1 }
        let e = by_rot.entry(r.rot).or_default();
        if r.serving { e.3 += 1; if r.won { e.2 += 1 } } else { e.1 += 1; if r.won { e.0 += 1 } }
    }

    // roster order: current lineup positions first, libero, then the rest by number
    let l = lineup_for(cfg, set.unwrap_or(rp.set));
    let mut order: Vec<i64> = l.pos.to_vec();
    if let Some(lib) = l.libero { order.push(lib); }
    let mut ids: Vec<i64> = p.keys().copied().collect();
    let number_of = |id: i64| players.iter().find(|x| x.id == id).map(|x| x.number).unwrap_or(999);
    ids.sort_by_key(|id| (order.iter().position(|&o| o == *id).unwrap_or(99), number_of(*id)));

    let players_json: Vec<Value> = ids
        .iter()
        .map(|id| {
            let s = &p[id];
            let pl = players.iter().find(|x| x.id == *id);
            let grades: Value = json!(s.grades);
            json!({
                "id": id,
                "number": pl.map(|x| x.number).unwrap_or(0),
                "name": pl.map(|x| x.name.clone()).unwrap_or_default(),
                "pos": pl.map(|x| x.position.clone()).unwrap_or_default(),
                "pts": s.pts, "n": s.n,
                "A": { "n": s.a_n, "k": s.a_k, "e": s.a_e, "blk": s.a_blk,
                       "pct": ratio(s.a_k - s.a_e - s.a_blk, s.a_n), "kpct": ratio(s.a_k, s.a_n) },
                "S": { "n": s.s_n, "ace": s.s_ace, "err": s.s_err, "pos": s.s_pos },
                "R": { "n": s.r_n, "perf": s.r_perf, "pos": s.r_pos, "err": s.r_err, "sum": s.r_sum,
                       "avg": ratio(s.r_sum, s.r_n), "pospct": ratio(s.r_pos, s.r_n), "perfpct": ratio(s.r_perf, s.r_n) },
                "B": { "pts": s.b_pts, "touch": s.b_touch, "err": s.b_err },
                "D": { "n": s.d_n, "good": s.d_good, "err": s.d_err },
                "E": { "n": s.e_n, "ast": s.e_ast, "err": s.e_err },
                "grades": grades,
            })
        })
        .collect();

    let by_rot_json: serde_json::Map<String, Value> = by_rot
        .iter()
        .map(|(k, v)| (k.to_string(), json!({"rot": k, "so": {"won": v.0, "n": v.1}, "brk": {"won": v.2, "n": v.3}})))
        .collect();

    json!({
        "set": set,
        "players": players_json,
        "team": {
            "us": us, "them": them,
            "ptsBy": pts_by, "errBy": err_by,
            "lostBy": { "oppKill": lost_opp_kill, "err": lost_err },
            "sideout": { "won": so.0, "n": so.1 },
            "brk": { "won": brk.0, "n": brk.1 },
            "byRot": by_rot_json,
            "grades": team_grades,
        },
        "state": {
            "set": rp.set, "us": rp.us, "them": rp.them, "sets": rp.sets,
            "sets_won": rp.sets_won, "sets_lost": rp.sets_lost, "finished": rp.finished,
        },
        "rallies": rallies.iter().map(|r| json!({"set": r.set, "rally": r.rally, "serving": r.serving, "won": r.won, "rot": r.rot, "us": r.us, "them": r.them})).collect::<Vec<_>>(),
    })
}

/// Flat CSV, one line per action, with the replayed context — readable by a
/// spreadsheet and by the openvolley toolchain after a trivial mapping.
pub fn export_csv(cfg: &MatchConfig, players: &[Player], actions: &[Action]) -> String {
    let rp = replay(cfg, actions);
    let num = |id: Option<i64>| id.and_then(|i| players.iter().find(|p| p.id == i)).map(|p| p.number.to_string()).unwrap_or_default();
    let mut out = String::from("seq,set,rally,score_us,score_them,serving,rotation_pos1,player,skill,grade,outcome,sub_out,sub_in\n");
    for r in &rp.rows {
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            r.seq, r.set, r.rally, r.us, r.them, if r.serving { 1 } else { 0 }, num(Some(r.rot)),
            num(r.player_id), r.skill, r.grade.clone().unwrap_or_default(), r.outcome.unwrap_or(""),
            num(r.sub_out), num(r.sub_in)
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> MatchConfig {
        let mut c = MatchConfig { first_serve_us: true, lineups: BTreeMap::new() };
        c.lineups.insert(1, Lineup { pos: [1, 2, 3, 4, 5, 6], libero: Some(7) });
        c
    }
    fn act(seq: i64, skill: &str, grade: &str, player: Option<i64>) -> Action {
        Action { id: seq, seq, skill: skill.into(), grade: Some(grade.into()), player_id: player, sub_out: None, sub_in: None }
    }

    #[test]
    fn serve_ace_scores_and_keeps_serve() {
        let st = replay(&cfg(), &[act(1, "S", "#", Some(1))]);
        assert_eq!((st.us, st.them), (1, 0));
        assert!(st.serving);
        assert_eq!(st.lineup, [1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn sideout_rotates() {
        // we serve and err → they serve; we receive and kill → rotate
        let st = replay(&cfg(), &[act(1, "S", "=", Some(1)), act(2, "R", "+", Some(7)), act(3, "A", "#", Some(2))]);
        assert_eq!((st.us, st.them), (1, 1));
        assert_eq!(st.lineup, [2, 3, 4, 5, 6, 1]);
        assert!(st.serving);
    }

    #[test]
    fn set_ends_at_25_with_two_clear_and_serve_alternates() {
        let mut a = vec![];
        for i in 0..25 { a.push(act(i, "S", "#", Some(1))); }
        let st = replay(&cfg(), &a);
        assert_eq!(st.set, 2);
        assert_eq!(st.sets.len(), 1);
        assert_eq!(st.sets[0].us, 25);
        assert!(!st.serving, "set 2 starts with the other side serving");
        assert_eq!(st.lineup, [1, 2, 3, 4, 5, 6], "lineup resets per set");
    }

    #[test]
    fn libero_override_follows_sub_and_undo_semantics() {
        let mut a = vec![];
        a.push(Action { id: 1, seq: 1, skill: "lib".into(), grade: None, player_id: None, sub_out: Some(5), sub_in: Some(7) });
        let st = replay(&cfg(), &a);
        assert_eq!(st.libero_for, Some(5));
        // the replaced player leaves the court → back to automatic
        a.push(Action { id: 2, seq: 2, skill: "sub".into(), grade: None, player_id: None, sub_out: Some(5), sub_in: Some(9) });
        let st = replay(&cfg(), &a);
        assert_eq!(st.libero_for, None);
        assert_eq!(st.lineup, [1, 2, 3, 4, 9, 6]);
        // libero out (no sub_in) → off until she is placed again
        a.push(Action { id: 3, seq: 3, skill: "lib".into(), grade: None, player_id: None, sub_out: None, sub_in: None });
        let st = replay(&cfg(), &a);
        assert!(st.libero_off);
        a.push(Action { id: 4, seq: 4, skill: "lib".into(), grade: None, player_id: None, sub_out: Some(6), sub_in: Some(7) });
        let st = replay(&cfg(), &a);
        assert!(!st.libero_off);
        assert_eq!(st.libero_for, Some(6));
    }

    #[test]
    fn catch_up_actions_change_score_rotation_and_serve_only() {
        let a = vec![
            Action { id: 1, seq: 1, skill: "adj".into(), grade: Some("#".into()), player_id: None, sub_out: None, sub_in: None },
            Action { id: 2, seq: 2, skill: "adj".into(), grade: Some("=".into()), player_id: None, sub_out: None, sub_in: None },
            Action { id: 3, seq: 3, skill: "rot".into(), grade: None, player_id: None, sub_out: None, sub_in: None },
            Action { id: 4, seq: 4, skill: "srv".into(), grade: Some("=".into()), player_id: None, sub_out: None, sub_in: None },
        ];
        let st = replay(&cfg(), &a[..1]);
        assert_eq!((st.us, st.them), (1, 0), "adj # is our point");
        let st = replay(&cfg(), &a);
        assert_eq!((st.us, st.them), (1, 1));
        assert_eq!(st.lineup, [2, 3, 4, 5, 6, 1]);
        assert!(!st.serving);
        assert!(st.rally_log.is_empty(), "catch-up points are not rallies");
    }

    #[test]
    fn deuce_needs_two() {
        let mut a = vec![];
        for i in 0..24 { a.push(act(i, "S", "#", Some(1))); }
        for i in 24..48 { a.push(act(i, "opp", "#", None)); } // 24:24
        a.push(act(48, "opp", "=", None)); // 25:24, not over
        let st = replay(&cfg(), &a);
        assert_eq!(st.set, 1);
        assert_eq!((st.us, st.them), (25, 24));
    }

    #[test]
    fn stats_count_kills_and_assists() {
        let a = vec![act(1, "S", "=", Some(1)), act(2, "R", "#", Some(7)), act(3, "E", "+", Some(4)), act(4, "A", "#", Some(2))];
        let players = vec![
            Player { id: 2, number: 3, name: "Mia".into(), position: "A".into() },
            Player { id: 4, number: 1, name: "Lena".into(), position: "Z".into() },
        ];
        let s = stats(&cfg(), &players, &a, None);
        let mia = s["players"].as_array().unwrap().iter().find(|p| p["id"] == 2).unwrap();
        assert_eq!(mia["A"]["k"], 1);
        assert_eq!(mia["pts"], 1);
        let lena = s["players"].as_array().unwrap().iter().find(|p| p["id"] == 4).unwrap();
        assert_eq!(lena["E"]["ast"], 1);
        assert_eq!(s["team"]["sideout"]["won"], 1);
        assert_eq!(s["team"]["brk"]["n"], 1);
    }

    /// Parity with the JS engine: fixture generated by web/scripts/fixture.mjs.
    #[test]
    fn parity_with_js_fixture() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/match.json");
        let Ok(raw) = std::fs::read_to_string(path) else { return };
        let fx: Value = serde_json::from_str(&raw).unwrap();
        let cfg: MatchConfig = serde_json::from_value(fx["config"].clone()).unwrap();
        let actions: Vec<Action> = serde_json::from_value(fx["actions"].clone()).unwrap();
        let players: Vec<Player> = serde_json::from_value(fx["players"].clone()).unwrap();
        let st = replay(&cfg, &actions);
        let exp = &fx["expected"];
        assert_eq!(st.set, exp["set"]);
        assert_eq!(st.us, exp["us"]);
        assert_eq!(st.them, exp["them"]);
        assert_eq!(json!(st.lineup), exp["lineup"]);
        assert_eq!(st.serving, exp["serving"]);
        assert_eq!(json!(st.sets), exp["sets"]);
        assert_eq!(json!(st.libero_for), exp["libero_for"]);
        assert_eq!(st.libero_off, exp["libero_off"]);
        let s = stats(&cfg, &players, &actions, None);
        assert_eq!(s["team"]["sideout"], exp["sideout"]);
        assert_eq!(s["team"]["brk"], exp["brk"]);
        assert_eq!(s["team"]["ptsBy"], exp["ptsBy"]);
        for ep in exp["players"].as_array().unwrap() {
            let p = s["players"].as_array().unwrap().iter().find(|p| p["id"] == ep["id"]).unwrap();
            assert_eq!(p["pts"], ep["pts"], "pts of {}", ep["id"]);
            assert_eq!(p["A"]["k"], ep["k"], "kills of {}", ep["id"]);
            assert_eq!(p["R"]["sum"], ep["rsum"], "reception of {}", ep["id"]);
            assert_eq!(p["E"]["ast"], ep["ast"], "assists of {}", ep["id"]);
        }
    }
}
