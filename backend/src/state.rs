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
    /// Start the database-backed scheduler once the shared application state is ready.
    pub fn start_scheduler(&self) {
        crate::scheduler::start(self.clone());
    }

    pub fn new(db: SqlitePool) -> Self {
        let worker_id = Uuid::new_v4().to_string();
        let events = EventBus::new(db.clone()).with_relay_from_env();
        Self {
            events,
            db,
            agents: Arc::new(AgentManager::new()),
            worker_id,
        }
    }
}
