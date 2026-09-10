use crate::agents::AgentManager;
use crate::events::EventBus;
use sqlx::SqlitePool;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub events: EventBus,
    pub agents: Arc<AgentManager>,
    pub login_failures: Arc<Mutex<HashMap<String, (u32, i64)>>>,
    pub worker_id: String,
}

impl AppState {
    pub fn new(db: SqlitePool) -> Self {
        Self {
            events: EventBus::new(db.clone()),
            db,
            agents: Arc::new(AgentManager::new()),
            login_failures: Arc::new(Mutex::new(HashMap::new())),
            worker_id: Uuid::new_v4().to_string(),
        }
    }
}
