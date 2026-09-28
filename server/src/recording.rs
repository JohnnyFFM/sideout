//! Recordings: one scout's log of a match on one device.
//!
//! A recording is an immutable initial state (`Base`: first serve, lineups,
//! roster, actions) plus numbered immutable edits. Nothing in it is ever
//! rewritten; an undo is an edit like any other. The current state is the
//! fold of the base and the edits up to N (`fold`), which yields exactly
//! what the engine replays: a match configuration and an action list.
//!
//! Content identity: the server parses, validates and re-serializes every
//! base and edit into one canonical form (fixed field order, explicit
//! nulls, integers only) and stores that string. A retry is identical when
//! its canonical string equals the stored one; a different string under the
//! same identity is an integrity error, never an overwrite.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::engine::{self, Action, Lineup, MatchConfig, Player};

pub const SCHEMA: i64 = 1;
pub const SKILLS: [&str; 12] = ["S", "R", "E", "A", "B", "D", "opp", "sub", "lib", "adj", "rot", "srv"];
pub const POSITIONS: [&str; 5] = ["Z", "A", "M", "D", "L"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecAction {
    pub seq: i64,
    pub skill: String,
    #[serde(default)]
    pub grade: Option<String>,
    #[serde(default)]
    pub player_id: Option<i64>,
    #[serde(default)]
    pub sub_out: Option<i64>,
    #[serde(default)]
    pub sub_in: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RosterEntry {
    pub id: i64,
    pub number: i64,
    pub name: String,
    pub position: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecLineup {
    pub pos: Vec<i64>,
    #[serde(default)]
    pub libero: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Base {
    pub schema: i64,
    pub first_serve_us: bool,
    /// set number → starting six; a set without an entry inherits the closest lower one
    pub lineups: BTreeMap<i64, RecLineup>,
    pub roster: Vec<RosterEntry>,
    pub actions: Vec<RecAction>,
}

/// One edit, flat on purpose: every field is present in the canonical form
/// (null where the op does not use it), so the serialization is stable and
/// a stray field on the wrong op is an error rather than silently ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    /// add | undo | lineup | first_serve | roster
    pub op: String,
    #[serde(default)]
    pub action: Option<RecAction>,
    #[serde(default)]
    pub seq: Option<i64>,
    #[serde(default)]
    pub set: Option<i64>,
    #[serde(default)]
    pub lineup: Option<RecLineup>,
    #[serde(default)]
    pub us: Option<bool>,
    #[serde(default)]
    pub player: Option<RosterEntry>,
}

/// the folded state the engine consumes
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub cfg: MatchConfig,
    pub roster: Vec<Player>,
    pub actions: Vec<Action>,
}

fn check_action(a: &RecAction) -> Result<(), String> {
    if a.seq < 1 {
        return Err("seq muss ≥ 1 sein".into());
    }
    if !SKILLS.contains(&a.skill.as_str()) {
        return Err(format!("Aktion unbekannt: {}", a.skill));
    }
    if let Some(g) = a.grade.as_deref() {
        if !engine::GRADES.contains(&g) {
            return Err(format!("Bewertung unbekannt: {g}"));
        }
    }
    match a.skill.as_str() {
        "S" | "R" | "E" | "A" | "B" | "D" => {
            if a.grade.is_none() || a.player_id.is_none() {
                return Err("Aktion braucht Bewertung und Spielerin".into());
            }
        }
        "opp" | "adj" | "srv" => {
            if !matches!(a.grade.as_deref(), Some("#") | Some("=")) {
                return Err(format!("{} braucht # oder =", a.skill));
            }
        }
        "sub" => {
            if a.sub_out.is_none() || a.sub_in.is_none() {
                return Err("Wechsel braucht raus und rein".into());
            }
        }
        _ => {}
    }
    for id in [a.player_id, a.sub_out, a.sub_in].into_iter().flatten() {
        if id < 1 {
            return Err("Spieler-ID ungültig".into());
        }
    }
    Ok(())
}

fn check_lineup(l: &RecLineup) -> Result<(), String> {
    if l.pos.len() != 6 {
        return Err("Aufstellung braucht genau 6 Positionen".into());
    }
    let mut ids = l.pos.clone();
    if let Some(lib) = l.libero {
        ids.push(lib);
    }
    if ids.iter().any(|i| *i < 1) {
        return Err("Aufstellung enthält eine ungültige Spieler-ID".into());
    }
    let mut uniq = ids.clone();
    uniq.sort();
    uniq.dedup();
    if uniq.len() != ids.len() {
        return Err("Eine Spielerin steht doppelt in der Aufstellung".into());
    }
    Ok(())
}

fn check_roster_entry(p: &RosterEntry) -> Result<(), String> {
    if p.id < 1 {
        return Err("Spieler-ID ungültig".into());
    }
    if !(0..=99).contains(&p.number) {
        return Err("Nummer muss zwischen 0 und 99 liegen".into());
    }
    if p.name.trim().is_empty() {
        return Err("Name fehlt".into());
    }
    if !POSITIONS.contains(&p.position.as_str()) {
        return Err("Position muss Z, A, M, D oder L sein".into());
    }
    Ok(())
}

/// Parse + validate a base; returns the struct and its canonical string.
pub fn parse_base(v: &serde_json::Value) -> Result<(Base, String), String> {
    let b: Base = serde_json::from_value(v.clone()).map_err(|e| format!("Anfangsstand ungültig: {e}"))?;
    if b.schema != SCHEMA {
        return Err(format!("Schema {} wird nicht unterstützt", b.schema));
    }
    for (set, l) in &b.lineups {
        if !(1..=5).contains(set) {
            return Err("Satz muss 1–5 sein".into());
        }
        check_lineup(l)?;
    }
    let mut seen = std::collections::BTreeSet::new();
    for p in &b.roster {
        check_roster_entry(p)?;
        if !seen.insert(p.id) {
            return Err("Spielerin doppelt im Kader".into());
        }
    }
    for a in &b.actions {
        check_action(a)?;
    }
    let canon = serde_json::to_string(&b).map_err(|e| e.to_string())?;
    Ok((b, canon))
}

/// Parse + validate an edit; returns the struct and its canonical string.
pub fn parse_edit(v: &serde_json::Value) -> Result<(Edit, String), String> {
    let e: Edit = serde_json::from_value(v.clone()).map_err(|e| format!("Edit ungültig: {e}"))?;
    let used: &[&str] = match e.op.as_str() {
        "add" => &["action"],
        "undo" => &["seq"],
        "lineup" => &["set", "lineup"],
        "first_serve" => &["us"],
        "roster" => &["player"],
        other => return Err(format!("Edit-Art unbekannt: {other}")),
    };
    let present = [
        ("action", e.action.is_some()),
        ("seq", e.seq.is_some()),
        ("set", e.set.is_some()),
        ("lineup", e.lineup.is_some()),
        ("us", e.us.is_some()),
        ("player", e.player.is_some()),
    ];
    for (name, is) in present {
        let wanted = used.contains(&name);
        if wanted && !is {
            return Err(format!("Edit {} braucht {name}", e.op));
        }
        if !wanted && is {
            return Err(format!("Edit {} kennt kein Feld {name}", e.op));
        }
    }
    if let Some(a) = &e.action {
        check_action(a)?;
    }
    if let Some(s) = e.seq {
        if s < 1 {
            return Err("seq muss ≥ 1 sein".into());
        }
    }
    if let Some(s) = e.set {
        if !(1..=5).contains(&s) {
            return Err("Satz muss 1–5 sein".into());
        }
    }
    if let Some(l) = &e.lineup {
        check_lineup(l)?;
    }
    if let Some(p) = &e.player {
        check_roster_entry(p)?;
    }
    let canon = serde_json::to_string(&e).map_err(|e| e.to_string())?;
    Ok((e, canon))
}

fn to_lineup(l: &RecLineup) -> Lineup {
    let mut pos = [0i64; 6];
    for (i, p) in l.pos.iter().take(6).enumerate() {
        pos[i] = *p;
    }
    Lineup { pos, libero: l.libero }
}

fn to_action(id: i64, a: &RecAction) -> Action {
    Action {
        id,
        seq: a.seq,
        skill: a.skill.clone(),
        grade: a.grade.clone(),
        player_id: a.player_id,
        sub_out: a.sub_out,
        sub_in: a.sub_in,
    }
}

fn to_player(p: &RosterEntry) -> Player {
    Player { id: p.id, number: p.number, name: p.name.clone(), position: p.position.clone() }
}

/// The state after the base and the given edits, in order. Lenient by
/// design: an undo whose target is not on top is a no-op, an add always
/// appends. What the scout recorded is what is replayed.
pub fn fold(base: &Base, edits: &[Edit]) -> Snapshot {
    let mut cfg = MatchConfig { first_serve_us: base.first_serve_us, lineups: BTreeMap::new() };
    for (set, l) in &base.lineups {
        cfg.lineups.insert(*set, to_lineup(l));
    }
    let mut roster: Vec<Player> = base.roster.iter().map(to_player).collect();
    let mut actions: Vec<Action> = base.actions.iter().map(|a| to_action(-a.seq, a)).collect();
    for (i, e) in edits.iter().enumerate() {
        let n = i as i64 + 1;
        match e.op.as_str() {
            "add" => {
                if let Some(a) = &e.action {
                    actions.push(to_action(n, a));
                }
            }
            "undo" => {
                if let (Some(seq), Some(last)) = (e.seq, actions.last()) {
                    if last.seq == seq {
                        actions.pop();
                    }
                }
            }
            "lineup" => {
                if let (Some(set), Some(l)) = (e.set, &e.lineup) {
                    cfg.lineups.insert(set, to_lineup(l));
                }
            }
            "first_serve" => {
                if let Some(us) = e.us {
                    cfg.first_serve_us = us;
                }
            }
            "roster" => {
                if let Some(p) = &e.player {
                    match roster.iter_mut().find(|r| r.id == p.id) {
                        Some(r) => *r = to_player(p),
                        None => roster.push(to_player(p)),
                    }
                }
            }
            _ => {}
        }
    }
    Snapshot { cfg, roster, actions }
}

/// A base from a folded state: what a copy (a fork) or the legacy migration stores.
pub fn base_from_snapshot(s: &Snapshot) -> Base {
    Base {
        schema: SCHEMA,
        first_serve_us: s.cfg.first_serve_us,
        lineups: s
            .cfg
            .lineups
            .iter()
            .map(|(k, l)| (*k, RecLineup { pos: l.pos.to_vec(), libero: l.libero }))
            .collect(),
        roster: s.roster.iter().map(|p| RosterEntry { id: p.id, number: p.number, name: p.name.clone(), position: p.position.clone() }).collect(),
        actions: s
            .actions
            .iter()
            .map(|a| RecAction { seq: a.seq, skill: a.skill.clone(), grade: a.grade.clone(), player_id: a.player_id, sub_out: a.sub_out, sub_in: a.sub_in })
            .collect(),
    }
}

/// Players for statistics: the team's roster as it is today, plus every
/// player the recording knows that the team no longer has (name and number
/// as recorded). Nothing in the log is ever dropped for lack of a player.
pub fn stat_players(team: &[Player], snap: &Snapshot) -> Vec<Player> {
    let mut out = team.to_vec();
    for p in &snap.roster {
        if !out.iter().any(|t| t.id == p.id) {
            out.push(p.clone());
        }
    }
    for a in &snap.actions {
        for id in [a.player_id, a.sub_out, a.sub_in].into_iter().flatten() {
            if !out.iter().any(|t| t.id == id) {
                out.push(Player { id, number: 0, name: format!("Unbekannt (ID {id})"), position: "?".into() });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn base_json() -> serde_json::Value {
        json!({ "schema": 1, "first_serve_us": true, "lineups": { "1": { "pos": [1, 2, 3, 4, 5, 6], "libero": 7 } },
                "roster": [{ "id": 1, "number": 1, "name": "Z", "position": "Z" }], "actions": [] })
    }

    #[test]
    fn canonical_form_ignores_key_order_and_whitespace() {
        let a = parse_edit(&json!({ "op": "add", "action": { "skill": "opp", "grade": "=", "seq": 1 } })).unwrap();
        let b = parse_edit(&serde_json::from_str(r#"{ "action": {"seq":1,  "grade":"=", "skill":"opp", "player_id": null}, "op":"add" }"#).unwrap()).unwrap();
        assert_eq!(a.1, b.1);
        let c = parse_edit(&json!({ "op": "add", "action": { "skill": "opp", "grade": "#", "seq": 1 } })).unwrap();
        assert_ne!(a.1, c.1);
    }

    #[test]
    fn stray_and_unknown_fields_are_refused() {
        assert!(parse_edit(&json!({ "op": "add", "action": { "skill": "opp", "grade": "=", "seq": 1 }, "seq": 3 })).is_err());
        assert!(parse_edit(&json!({ "op": "undo", "seq": 2, "extra": 1 })).is_err());
        assert!(parse_edit(&json!({ "op": "undo" })).is_err());
        assert!(parse_edit(&json!({ "op": "fly" })).is_err());
        assert!(parse_base(&json!({ "schema": 2, "first_serve_us": true, "lineups": {}, "roster": [], "actions": [] })).is_err());
        assert!(parse_edit(&json!({ "op": "add", "action": { "skill": "A", "grade": "#", "seq": 1 } })).is_err(), "player missing");
    }

    #[test]
    fn fold_applies_adds_and_targeted_undo() {
        let (base, _) = parse_base(&base_json()).unwrap();
        let e = |v: serde_json::Value| parse_edit(&v).unwrap().0;
        let edits = vec![
            e(json!({ "op": "add", "action": { "seq": 1, "skill": "opp", "grade": "=" } })),
            e(json!({ "op": "add", "action": { "seq": 2, "skill": "opp", "grade": "#" } })),
            e(json!({ "op": "undo", "seq": 1 })), // not on top: no-op
            e(json!({ "op": "undo", "seq": 2 })),
            e(json!({ "op": "first_serve", "us": false })),
            e(json!({ "op": "roster", "player": { "id": 9, "number": 9, "name": "Neu", "position": "A" } })),
            e(json!({ "op": "lineup", "set": 2, "lineup": { "pos": [6, 5, 4, 3, 2, 1], "libero": null } })),
        ];
        let s = fold(&base, &edits);
        assert_eq!(s.actions.len(), 1);
        assert_eq!(s.actions[0].seq, 1);
        assert!(!s.cfg.first_serve_us);
        assert_eq!(s.roster.len(), 2);
        assert_eq!(s.cfg.lineups[&2].pos, [6, 5, 4, 3, 2, 1]);
        let st = engine::replay(&s.cfg, &s.actions);
        assert_eq!(st.us, 1);
        // a copy of the state is a base again, and folds to the same thing
        let b2 = base_from_snapshot(&s);
        let s2 = fold(&b2, &[]);
        assert_eq!(s2.actions.len(), 1);
        assert_eq!(s2.cfg.lineups.len(), 2);
    }
}
