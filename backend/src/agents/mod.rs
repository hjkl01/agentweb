pub mod adapter;
pub mod codex;
pub mod generic;
pub mod opencode;
pub mod pi;
pub mod process;

pub use adapter::{AgentAdapter, AgentConfig};
use codex::CodexAdapter;
use generic::GenericAdapter;
use opencode::OpenCodeAdapter;
use pi::PiAdapter;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

pub struct AgentManager {
    adapters: Mutex<HashMap<String, Arc<dyn AgentAdapter>>>,
}
impl Default for AgentManager {
    fn default() -> Self {
        Self {
            adapters: Mutex::new(HashMap::new()),
        }
    }
}
impl AgentManager {
    pub fn new() -> Self {
        Self::default()
    }
    pub async fn adapter(&self, kind: &str) -> Arc<dyn AgentAdapter> {
        let mut map = self.adapters.lock().await;
        if let Some(a) = map.get(kind) {
            return a.clone();
        }
        let adapter: Arc<dyn AgentAdapter> = match kind {
            "codex" => Arc::new(CodexAdapter::new()),
            "opencode" => Arc::new(OpenCodeAdapter::new()),
            "pi" => Arc::new(PiAdapter::new()),
            _ => Arc::new(GenericAdapter::new()),
        };
        map.insert(kind.to_owned(), adapter.clone());
        adapter
    }
}
