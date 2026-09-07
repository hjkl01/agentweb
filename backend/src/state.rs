use crate::agents::AgentManager;
use crate::events::EventBus;
use sqlx::SqlitePool;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub events: EventBus,
    pub agents: Arc<AgentManager>,
}

impl AppState {
    pub fn new(db: SqlitePool) -> Self {
        Self {
            db,
            events: EventBus::new(),
            agents: Arc::new(AgentManager::new()),
        }
    }
}
