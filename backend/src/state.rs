use crate::agents::AgentManager;
use crate::events::EventBus;
use sqlx::SqlitePool;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub events: EventBus,
    pub agents: Arc<AgentManager>,
    pub login_failures: Arc<Mutex<HashMap<String, (u32, i64)>>>,
}

impl AppState {
    pub fn new(db: SqlitePool) -> Self {
        Self { db, events: EventBus::new(), agents: Arc::new(AgentManager::new()), login_failures: Arc::new(Mutex::new(HashMap::new())) }
    }
}
