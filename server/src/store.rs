//! Shared persistence helpers: audit lines, row → JSON shapes, and the
//! loaders that feed the engine.

use serde_json::{json, Value};
use sqlx::{Row, SqliteConnection};

use crate::engine::{Action, Lineup, MatchConfig, Player};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

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

pub fn action_json(r: &sqlx::sqlite::SqliteRow) -> Value {
    json!({
        "id": r.get::<i64, _>("id"),
        "seq": r.get::<i64, _>("seq"),
        "set_no": r.get::<i64, _>("set_no"),
        "skill": r.get::<String, _>("skill"),
        "grade": r.get::<Option<String>, _>("grade"),
        "player_id": r.get::<Option<i64>, _>("player_id"),
        "sub_out": r.get::<Option<i64>, _>("sub_out"),
        "sub_in": r.get::<Option<i64>, _>("sub_in"),
        "cid": r.get::<Option<String>, _>("cid"),
        "created_at": r.get::<String, _>("created_at"),
    })
}

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

pub async fn load_config(state: &AppState, match_id: i64, first_serve: &str) -> ApiResult<MatchConfig> {
    let mut conn = state.db.acquire().await?;
    load_config_conn(&mut conn, match_id, first_serve).await
}

pub async fn load_config_conn(conn: &mut SqliteConnection, match_id: i64, first_serve: &str) -> ApiResult<MatchConfig> {
    let rows = sqlx::query(
        "SELECT set_no, pos1, pos2, pos3, pos4, pos5, pos6, libero_id FROM lineups WHERE match_id = ? ORDER BY set_no",
    )
    .bind(match_id)
    .fetch_all(conn)
    .await?;
    let mut cfg = MatchConfig {
        first_serve_us: first_serve == "us",
        lineups: Default::default(),
    };
    for r in rows {
        cfg.lineups.insert(
            r.get::<i64, _>("set_no"),
            Lineup {
                pos: [
                    r.get("pos1"),
                    r.get("pos2"),
                    r.get("pos3"),
                    r.get("pos4"),
                    r.get("pos5"),
                    r.get("pos6"),
                ],
                libero: r.get("libero_id"),
            },
        );
    }
    Ok(cfg)
}

pub async fn load_actions(state: &AppState, match_id: i64) -> ApiResult<Vec<Action>> {
    let mut conn = state.db.acquire().await?;
    load_actions_conn(&mut conn, match_id).await
}

pub async fn load_actions_conn(conn: &mut SqliteConnection, match_id: i64) -> ApiResult<Vec<Action>> {
    let rows = sqlx::query(
        "SELECT id, seq, set_no, skill, grade, player_id, sub_out, sub_in FROM actions WHERE match_id = ? ORDER BY seq",
    )
    .bind(match_id)
    .fetch_all(conn)
    .await?;
    Ok(rows
        .iter()
        .map(|r| Action {
            id: r.get("id"),
            seq: r.get("seq"),
            skill: r.get("skill"),
            grade: r.get("grade"),
            player_id: r.get("player_id"),
            sub_out: r.get("sub_out"),
            sub_in: r.get("sub_in"),
        })
        .collect())
}

pub fn lineups_json(cfg: &MatchConfig) -> Value {
    let mut m = serde_json::Map::new();
    for (set, l) in &cfg.lineups {
        m.insert(set.to_string(), json!({ "pos": l.pos, "libero": l.libero }));
    }
    Value::Object(m)
}
