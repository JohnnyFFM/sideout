//! One EventSource per client; every committed mutation lands here within
//! milliseconds. Events are filtered to the subscriber's team.

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::auth::CurrentUser;
use crate::state::AppState;

pub async fn sse(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let team_id = user.team_id;
    let rx = state.events.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(move |msg| {
        msg.ok().filter(|m| m.team_id == team_id).map(|m| {
            Ok(Event::default()
                .event("mutation")
                .data(serde_json::to_string(&m).unwrap_or_default()))
        })
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(25)).text("ping"))
}
