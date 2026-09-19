//! Committed-mutation broadcast → SSE. Every write publishes here; clients
//! hold one EventSource and refetch what they show. The payload is a
//! notification, never the data.

use serde::Serialize;
use tokio::sync::broadcast;

#[derive(Debug, Clone, Serialize)]
pub struct EventMsg {
    /// Tenant scope — the SSE handler drops events for other teams.
    pub team_id: i64,
    pub entity: String, // "match" | "action" | "player" | "team"
    pub id: i64,
    /// match version, or the action seq for entity "action"
    pub version: i64,
    pub action: String,
    pub actor: String,
}

#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<EventMsg>,
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self { tx }
    }
    pub fn publish(&self, msg: EventMsg) {
        let _ = self.tx.send(msg);
    }
    pub fn subscribe(&self) -> broadcast::Receiver<EventMsg> {
        self.tx.subscribe()
    }
}
