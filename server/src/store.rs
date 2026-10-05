//! Shared persistence helpers: audit lines, row → JSON shapes, the
//! recording loaders that feed the engine, and the one-time migration of
//! the old shared action log into recordings.

use serde_json::{json, Value};
use sqlx::{Row, SqliteConnection, SqlitePool};

use crate::auth::Role;
use crate::engine::{self, Lineup, MatchConfig, Player};
use crate::error::{ApiError, ApiResult};
use crate::recording::{self, Base, Edit, Snapshot};
use crate::state::AppState;

/// minutes after the newest edit during which a recording counts as "being scouted right now"
pub const ACTIVE_MINUTES: i64 = 10;

pub async fn audit(
    state: &AppState,
    team_id: i64,
    entity: &str,
    entity_id: i64,
    action: &str,
    detail: &str,
    user_id: Option<i64>,
) -> ApiResult<()> {
    let mut conn = state.dbw.acquire().await?;
    audit_conn(&mut conn, team_id, entity, entity_id, action, detail, user_id).await
}

/// the same audit line on an explicit connection (a write transaction must
/// not touch the pool it came from: the pool has one connection)
pub async fn audit_conn(
    conn: &mut SqliteConnection,
    team_id: i64,
    entity: &str,
    entity_id: i64,
    action: &str,
    detail: &str,
    user_id: Option<i64>,
) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO audit_log (team_id, entity, entity_id, action, detail, user_id)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(team_id)
    .bind(entity)
    .bind(entity_id)
    .bind(action)
    .bind(detail)
    .bind(user_id)
    .execute(conn)
    .await?;
    Ok(())
}

pub fn match_json(r: &sqlx::sqlite::SqliteRow) -> Value {
    json!({
        "id": r.get::<i64, _>("id"),
        "opponent": r.get::<String, _>("opponent"),
        "date": r.get::<String, _>("date"),
        "time": r.get::<Option<String>, _>("time"),
        "hall": r.get::<String, _>("hall"),
        "home": r.get::<i64, _>("home") != 0,
        "first_serve": r.get::<String, _>("first_serve"),
        "status": r.get::<String, _>("status"),
        "notes": r.get::<String, _>("notes"),
        "version": r.get::<i64, _>("version"),
        "archived": r.get::<i64, _>("archived") != 0,
        "selected": r.get::<Option<String>, _>("selected_recording"),
        "selection_rev": r.get::<i64, _>("selection_rev"),
        "created_at": r.get::<String, _>("created_at"),
        "updated_at": r.get::<String, _>("updated_at"),
    })
}

pub fn player_json(r: &sqlx::sqlite::SqliteRow) -> Value {
    json!({
        "id": r.get::<i64, _>("id"),
        "number": r.get::<i64, _>("number"),
        "name": r.get::<String, _>("name"),
        "position": r.get::<String, _>("position"),
        "active": r.get::<i64, _>("active") != 0,
    })
}

pub fn action_json(a: &engine::Action) -> Value {
    json!({
        "id": a.id,
        "seq": a.seq,
        "skill": a.skill,
        "grade": a.grade,
        "player_id": a.player_id,
        "sub_out": a.sub_out,
        "sub_in": a.sub_in,
    })
}

pub fn lineups_json(cfg: &MatchConfig) -> Value {
    let mut m = serde_json::Map::new();
    for (set, l) in &cfg.lineups {
        m.insert(set.to_string(), json!({ "pos": l.pos, "libero": l.libero }));
    }
    Value::Object(m)
}

pub fn snapshot_json(s: &Snapshot) -> Value {
    json!({
        "first_serve_us": s.cfg.first_serve_us,
        "lineups": lineups_json(&s.cfg),
        "roster": s.roster,
        "actions": s.actions.iter().map(action_json).collect::<Vec<_>>(),
    })
}

/// the compact state a list or a recording summary shows
pub fn state_summary(st: &engine::State) -> Value {
    json!({
        "set": st.set, "us": st.us, "them": st.them, "sets": st.sets,
        "sets_won": st.sets_won, "sets_lost": st.sets_lost, "finished": st.finished, "last_seq": st.last_seq,
    })
}

// ---------------------------------------------------------------- matches

/// a match of the caller's active team
pub async fn fetch_match_row(state: &AppState, team_id: i64, id: i64) -> ApiResult<sqlx::sqlite::SqliteRow> {
    let mut conn = state.db.acquire().await?;
    fetch_match_row_conn(&mut conn, team_id, id).await
}

pub async fn fetch_match_row_conn(conn: &mut SqliteConnection, team_id: i64, id: i64) -> ApiResult<sqlx::sqlite::SqliteRow> {
    sqlx::query("SELECT * FROM matches WHERE id = ? AND team_id = ?")
        .bind(id)
        .bind(team_id)
        .fetch_optional(conn)
        .await?
        .ok_or(ApiError::NotFound)
}

/// a match by id alone, whatever team the session has selected: uploads
/// resolve the team from the match and check membership there
pub async fn fetch_match_any_conn(conn: &mut SqliteConnection, id: i64) -> ApiResult<sqlx::sqlite::SqliteRow> {
    sqlx::query("SELECT * FROM matches WHERE id = ?").bind(id).fetch_optional(conn).await?.ok_or(ApiError::NotFound)
}

/// the caller's role in a given team (None = not a member)
pub async fn team_role_conn(conn: &mut SqliteConnection, user_id: i64, team_id: i64) -> ApiResult<Option<Role>> {
    let r = sqlx::query("SELECT role FROM memberships WHERE user_id = ? AND team_id = ?")
        .bind(user_id)
        .bind(team_id)
        .fetch_optional(conn)
        .await?;
    Ok(r.map(|r| Role::parse(&r.get::<String, _>("role"))))
}

pub async fn load_players(state: &AppState, team_id: i64) -> ApiResult<Vec<Player>> {
    let mut conn = state.db.acquire().await?;
    load_players_conn(&mut conn, team_id).await
}

pub async fn load_players_conn(conn: &mut SqliteConnection, team_id: i64) -> ApiResult<Vec<Player>> {
    let rows = sqlx::query("SELECT id, number, name, position FROM players WHERE team_id = ? ORDER BY number")
        .bind(team_id)
        .fetch_all(conn)
        .await?;
    Ok(rows
        .iter()
        .map(|r| Player {
            id: r.get("id"),
            number: r.get("number"),
            name: r.get("name"),
            position: r.get("position"),
        })
        .collect())
}

/// the planning of a match: first serve and the lineups entered before
/// scouting started (a new recording takes them as its initial state)
pub async fn load_planning_conn(conn: &mut SqliteConnection, match_id: i64, first_serve: &str) -> ApiResult<MatchConfig> {
    let rows = sqlx::query(
        "SELECT set_no, pos1, pos2, pos3, pos4, pos5, pos6, libero_id FROM lineups WHERE match_id = ? ORDER BY set_no",
    )
    .bind(match_id)
    .fetch_all(conn)
    .await?;
    let mut cfg = MatchConfig { first_serve_us: first_serve == "us", lineups: Default::default() };
    for r in rows {
        cfg.lineups.insert(
            r.get::<i64, _>("set_no"),
            Lineup {
                pos: [r.get("pos1"), r.get("pos2"), r.get("pos3"), r.get("pos4"), r.get("pos5"), r.get("pos6")],
                libero: r.get("libero_id"),
            },
        );
    }
    Ok(cfg)
}

// ------------------------------------------------------------- recordings

#[derive(Debug, Clone)]
pub struct RecMeta {
    pub id: String,
    pub match_id: i64,
    pub team_id: i64,
    pub user_id: Option<i64>,
    pub user_name: Option<String>,
    pub device_id: String,
    pub device_label: String,
    pub origin_id: Option<String>,
    pub origin_n: Option<i64>,
    pub created_at: String,
    pub last_write: String,
    /// number of edits stored (dense, so also the highest edit number)
    pub n: i64,
    /// newest edit younger than ACTIVE_MINUTES (never for an import)
    pub active: bool,
    /// a transport of past work (old queue, file, legacy migration), not live scouting
    pub imported: bool,
    /// hidden by a coach or its creator; rows kept
    pub deleted: bool,
    /// a pure prefix of the result's chain of copies: every edit went into the
    /// copy, nothing of its own. Not listed. Set by `mark_superseded`.
    pub superseded: bool,
}

fn rec_meta(r: &sqlx::sqlite::SqliteRow) -> RecMeta {
    RecMeta {
        id: r.get("id"),
        match_id: r.get("match_id"),
        team_id: r.get("team_id"),
        user_id: r.get("user_id"),
        user_name: r.get("user_name"),
        device_id: r.get("device_id"),
        device_label: r.get("device_label"),
        origin_id: r.get("origin_id"),
        origin_n: r.get("origin_n"),
        created_at: r.get("created_at"),
        last_write: r.get("last_write"),
        n: r.get("n"),
        active: r.get::<i64, _>("active") != 0,
        imported: r.get::<i64, _>("imported") != 0,
        deleted: r.get::<i64, _>("deleted") != 0,
        superseded: false,
    }
}

const REC_SELECT: &str = "SELECT r.*, u.display_name AS user_name,
        (SELECT count(*) FROM edits e WHERE e.recording_id = r.id) AS n,
        (r.imported = 0 AND r.last_write > datetime('now', '-10 minutes')) AS active
     FROM recordings r LEFT JOIN users u ON u.id = r.user_id";
const _: () = assert!(ACTIVE_MINUTES == 10);

pub fn rec_meta_json(m: &RecMeta) -> Value {
    json!({
        "id": m.id, "match_id": m.match_id, "team_id": m.team_id, "user_id": m.user_id, "user": m.user_name,
        "device_id": m.device_id, "device": m.device_label,
        "origin_id": m.origin_id, "origin_n": m.origin_n,
        "created_at": m.created_at, "last_write": m.last_write, "n": m.n, "active": m.active, "imported": m.imported, "deleted": m.deleted,
        "superseded": m.superseded,
    })
}

pub async fn recording_meta_conn(conn: &mut SqliteConnection, rid: &str) -> ApiResult<Option<RecMeta>> {
    let r = sqlx::query(&format!("{REC_SELECT} WHERE r.id = ?")).bind(rid).fetch_optional(conn).await?;
    Ok(r.as_ref().map(rec_meta))
}

/// Is `got` the folded state of recording `oid` after exactly its first `n`
/// edits, `n` being all it has? Serve, lineups, actions and the known
/// players must match; the roster may only have grown. What a copy claims
/// with `origin_id`/`origin_n` is never trusted without this.
pub async fn continues_exactly(conn: &mut SqliteConnection, oid: &str, n: i64, got: &Base) -> ApiResult<bool> {
    let Some((ob, oe)) = load_recording_conn(conn, oid).await? else { return Ok(false) };
    if n < 0 || oe.len() != n as usize {
        return Ok(false);
    }
    let want = recording::base_from_snapshot(&recording::fold(&ob, &oe));
    Ok(want.first_serve_us == got.first_serve_us
        && want.lineups == got.lineups
        && want.actions == got.actions
        && want.roster.iter().all(|p| got.roster.iter().any(|q| q == p)))
}

/// Marks what the result makes redundant: walking the result's copies back
/// (`origin_id`), a recording whose edits all went into the copy — verified
/// against the stored data, not the claim — carries nothing of its own and
/// is folded away. One further tap into it ends that.
pub async fn mark_superseded_conn(conn: &mut SqliteConnection, recs: &mut [RecMeta], selected: Option<&str>) -> ApiResult<()> {
    let mut cur = selected.map(str::to_string);
    let mut seen = std::collections::HashSet::new();
    while let Some(id) = cur.take() {
        if !seen.insert(id.clone()) {
            break;
        }
        let Some((oid, on)) = recs.iter().find(|r| r.id == id).and_then(|c| Some((c.origin_id.clone()?, c.origin_n?))) else { break };
        let Some(oi) = recs.iter().position(|r| r.id == oid) else { break };
        if recs[oi].n != on {
            break;
        }
        let Some((copy_base, _)) = load_recording_conn(conn, &id).await? else { break };
        if !continues_exactly(conn, &oid, on, &copy_base).await? {
            break;
        }
        recs[oi].superseded = true;
        cur = Some(oid);
    }
    Ok(())
}

pub async fn recordings_of_conn(conn: &mut SqliteConnection, match_id: i64) -> ApiResult<Vec<RecMeta>> {
    let rows = sqlx::query(&format!("{REC_SELECT} WHERE r.match_id = ? AND r.deleted = 0 ORDER BY r.created_at, r.id")).bind(match_id).fetch_all(conn).await?;
    Ok(rows.iter().map(rec_meta).collect())
}

/// base + edits of a recording, parsed from their canonical strings
pub async fn load_recording_conn(conn: &mut SqliteConnection, rid: &str) -> ApiResult<Option<(Base, Vec<Edit>)>> {
    let Some(r) = sqlx::query("SELECT base FROM recordings WHERE id = ?").bind(rid).fetch_optional(&mut *conn).await? else {
        return Ok(None);
    };
    let base: Base = serde_json::from_str(&r.get::<String, _>("base")).map_err(|e| ApiError::Internal(format!("base {rid}: {e}")))?;
    let rows = sqlx::query("SELECT body FROM edits WHERE recording_id = ? ORDER BY n").bind(rid).fetch_all(&mut *conn).await?;
    let mut edits = Vec::with_capacity(rows.len());
    for e in rows {
        edits.push(serde_json::from_str::<Edit>(&e.get::<String, _>("body")).map_err(|e| ApiError::Internal(format!("edit {rid}: {e}")))?);
    }
    Ok(Some((base, edits)))
}

pub async fn snapshot_of_conn(conn: &mut SqliteConnection, rid: &str) -> ApiResult<Option<Snapshot>> {
    Ok(load_recording_conn(conn, rid).await?.map(|(b, e)| recording::fold(&b, &e)))
}

/// what a fresh recording of this match starts from: the planning and the
/// active roster (plus inactive players the planning references)
pub async fn planning_snapshot_conn(conn: &mut SqliteConnection, row: &sqlx::sqlite::SqliteRow) -> ApiResult<Snapshot> {
    let id: i64 = row.get("id");
    let team_id: i64 = row.get("team_id");
    let cfg = load_planning_conn(conn, id, &row.get::<String, _>("first_serve")).await?;
    let rows = sqlx::query("SELECT id, number, name, position, active FROM players WHERE team_id = ? ORDER BY number")
        .bind(team_id)
        .fetch_all(conn)
        .await?;
    let referenced: Vec<i64> = cfg.lineups.values().flat_map(|l| l.pos.iter().copied().chain(l.libero)).collect();
    let roster = rows
        .iter()
        .filter(|r| r.get::<i64, _>("active") != 0 || referenced.contains(&r.get::<i64, _>("id")))
        .map(|r| Player { id: r.get("id"), number: r.get("number"), name: r.get("name"), position: r.get("position") })
        .collect();
    Ok(Snapshot { cfg, roster, actions: vec![] })
}

/// the match result: the selected recording's state, or the planning when
/// nothing has been recorded yet
pub async fn selected_snapshot_conn(conn: &mut SqliteConnection, row: &sqlx::sqlite::SqliteRow) -> ApiResult<(Option<String>, Snapshot)> {
    if let Some(sel) = row.get::<Option<String>, _>("selected_recording") {
        if let Some(s) = snapshot_of_conn(conn, &sel).await? {
            return Ok((Some(sel), s));
        }
    }
    Ok((None, planning_snapshot_conn(conn, row).await?))
}

pub async fn selected_snapshot(state: &AppState, row: &sqlx::sqlite::SqliteRow) -> ApiResult<(Option<String>, Snapshot)> {
    let mut conn = state.db.acquire().await?;
    selected_snapshot_conn(&mut conn, row).await
}

/// `matches.status` follows the selected recording: done when its replay is
/// finished, live while it exists, planned without one. Returns whether the
/// row changed (the caller publishes).
pub async fn refresh_status_conn(conn: &mut SqliteConnection, match_id: i64) -> ApiResult<bool> {
    let row = fetch_match_any_conn(conn, match_id).await?;
    let (sel, snap) = selected_snapshot_conn(conn, &row).await?;
    let new_status = match sel {
        None => "planned",
        Some(_) => {
            if engine::replay(&snap.cfg, &snap.actions).finished { "done" } else { "live" }
        }
    };
    if new_status == row.get::<String, _>("status") {
        return Ok(false);
    }
    sqlx::query("UPDATE matches SET status = ?, version = version + 1, updated_at = datetime('now') WHERE id = ?")
        .bind(new_status)
        .bind(match_id)
        .execute(conn)
        .await?;
    Ok(true)
}

// --------------------------------------------------------- legacy import

/// One-time move of the old shared action log (`legacy_actions`, the table
/// before migration 0008) into one recording per match, selected as the
/// match result. Idempotent: every migrated match's rows are deleted, so a
/// restart finds nothing left to do.
pub async fn migrate_legacy(dbw: &SqlitePool) -> ApiResult<usize> {
    let mids: Vec<i64> = sqlx::query("SELECT DISTINCT match_id FROM legacy_actions ORDER BY match_id")
        .fetch_all(dbw)
        .await?
        .iter()
        .map(|r| r.get("match_id"))
        .collect();
    let mut done = 0;
    for mid in mids {
        let mut tx = dbw.begin().await?;
        let rid = format!("legacy-{mid}");
        let row = sqlx::query("SELECT * FROM matches WHERE id = ?").bind(mid).fetch_optional(&mut *tx).await?;
        let Some(row) = row else {
            sqlx::query("DELETE FROM legacy_actions WHERE match_id = ?").bind(mid).execute(&mut *tx).await?;
            tx.commit().await?;
            continue;
        };
        let exists = sqlx::query("SELECT 1 FROM recordings WHERE id = ?").bind(&rid).fetch_optional(&mut *tx).await?.is_some();
        if !exists {
            let team_id: i64 = row.get("team_id");
            let mut snap = planning_snapshot_conn(&mut tx, &row).await?;
            // the recording's roster: everyone the log or the lineups mention, as the team knows them today
            let team = load_players_conn(&mut tx, team_id).await?;
            let rows = sqlx::query("SELECT id, seq, skill, grade, player_id, sub_out, sub_in FROM legacy_actions WHERE match_id = ? ORDER BY seq")
                .bind(mid)
                .fetch_all(&mut *tx)
                .await?;
            snap.actions = rows
                .iter()
                .map(|r| engine::Action {
                    id: r.get("id"),
                    seq: r.get("seq"),
                    skill: r.get("skill"),
                    grade: r.get("grade"),
                    player_id: r.get("player_id"),
                    sub_out: r.get("sub_out"),
                    sub_in: r.get("sub_in"),
                })
                .collect();
            snap.roster = recording::stat_players(&team, &snap);
            let base = recording::base_from_snapshot(&snap);
            let canon = serde_json::to_string(&base).map_err(|e| ApiError::Internal(e.to_string()))?;
            sqlx::query(
                "INSERT INTO recordings (id, match_id, team_id, user_id, device_id, device_label, base, created_at, last_write, imported)
                 VALUES (?, ?, ?, ?, 'legacy', 'Import', ?, ?, ?, 1)",
            )
            .bind(&rid)
            .bind(mid)
            .bind(team_id)
            .bind(row.get::<Option<i64>, _>("created_by"))
            .bind(&canon)
            .bind(row.get::<String, _>("created_at"))
            .bind(row.get::<String, _>("updated_at"))
            .execute(&mut *tx)
            .await?;
            sqlx::query("UPDATE matches SET selected_recording = ?, selection_rev = selection_rev + 1 WHERE id = ? AND selected_recording IS NULL")
                .bind(&rid)
                .bind(mid)
                .execute(&mut *tx)
                .await?;
            audit_conn(&mut tx, team_id, "match", mid, "recording_import", &rid, None).await?;
        }
        sqlx::query("DELETE FROM legacy_actions WHERE match_id = ?").bind(mid).execute(&mut *tx).await?;
        tx.commit().await?;
        done += 1;
    }
    Ok(done)
}
