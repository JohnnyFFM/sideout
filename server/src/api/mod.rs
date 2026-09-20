pub mod account;
pub mod auth_routes;
pub mod events;
pub mod matches;
pub mod team;
#[cfg(test)]
mod tests;

use axum::routing::{delete, get, patch, post, put};
use axum::Router;

use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        // auth (no session required)
        .route("/auth/register-team", post(auth_routes::register_team))
        .route("/auth/join", post(auth_routes::join))
        .route("/auth/login", post(auth_routes::login))
        .route("/auth/logout", post(auth_routes::logout))
        .route("/config", get(auth_routes::config))
        .route("/me", get(auth_routes::me).patch(account::patch_me))
        .route("/me/password", post(account::change_password))
        // team & roster
        .route("/team", patch(team::patch_team))
        .route("/team/rotate-code", post(team::rotate_code))
        .route("/team/members/{id}", patch(team::patch_member).delete(team::delete_member))
        .route("/teams", post(team::create_team))
        .route("/teams/join", post(team::join_team))
        .route("/teams/switch", post(team::switch_team))
        // per-team (any of my teams, not only the active one): the Teams page
        .route("/teams/{id}", get(account::get_team).patch(team::patch_team_by_id))
        .route("/teams/{id}/membership", delete(account::leave_team))
        .route("/teams/{id}/rotate-code", post(team::rotate_code_by_id))
        .route("/teams/{id}/members/{uid}", patch(team::patch_member_by_id).delete(team::delete_member_by_id))
        .route("/players", get(team::list_players).post(team::create_player))
        .route("/players/{id}", patch(team::patch_player).delete(team::delete_player))
        // matches
        .route("/matches", get(matches::list).post(matches::create))
        .route("/matches/{id}", get(matches::get_one).patch(matches::update).delete(matches::remove))
        .route("/matches/{id}/lineups/{set}", put(matches::put_lineup))
        .route("/matches/{id}/actions", post(matches::add_action))
        .route("/matches/{id}/actions/last", delete(matches::undo_action))
        .route("/matches/{id}/state", get(matches::state))
        .route("/matches/{id}/stats", get(matches::stats))
        .route("/matches/{id}/export.csv", get(matches::export_csv))
        .route("/season/stats", get(matches::season))
        // live events
        .route("/events", get(events::sse))
        .layer(axum::middleware::from_fn(crate::auth::csrf_layer))
        .with_state(state)
}
