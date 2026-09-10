use crate::agents::AgentManager;
use crate::events::EventBus;
use sqlx::SqlitePool;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub events: EventBus,
    pub agents: Arc<AgentManager>,
    pub worker_id: String,
}

impl AppState {
    pub fn new(db: SqlitePool) -> Self {
        Self {
            events: EventBus::new(db.clone()),
            db,
            agents: Arc::new(AgentManager::new()),
            worker_id: Uuid::new_v4().to_string(),
        }
    }
}
