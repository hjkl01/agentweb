pub mod adapter;
pub mod codex;
pub mod codex_events;
pub mod commands;
pub mod config;
pub mod definition;
pub mod event_parser;
pub mod generic;
pub mod models;
pub mod opencode;
pub mod pi;
pub mod pi_events;
pub mod process;
pub mod session;

pub use adapter::{AgentAdapter, AgentConfig};
use codex::CodexAdapter;
use generic::GenericAdapter;
use opencode::OpenCodeAdapter;
use pi::PiAdapter;
use anyhow::Result;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

pub struct AgentManager { adapters: Mutex<HashMap<String, Arc<dyn AgentAdapter>>> }
impl Default for AgentManager { fn default() -> Self { Self { adapters: Mutex::new(HashMap::new()) } } }
impl AgentManager {
    pub fn new() -> Self { Self::default() }

    pub async fn adapter(&self, kind: &str) -> Arc<dyn AgentAdapter> {
        let mut map = self.adapters.lock().await;
        if let Some(a) = map.get(kind) { return a.clone(); }
        let adapter: Arc<dyn AgentAdapter> = match kind {
            "codex" => Arc::new(CodexAdapter::new()),
            "opencode" => Arc::new(OpenCodeAdapter::new()),
            "pi" => Arc::new(PiAdapter::new()),
            _ => Arc::new(GenericAdapter::new()),
        };
        map.insert(kind.to_owned(), adapter.clone());
        adapter
    }

    pub async fn interrupt(&self, session_id: &str) -> Result<()> {
        let adapters = self.adapters.lock().await.values().cloned().collect::<Vec<_>>();
        for adapter in adapters { adapter.interrupt(session_id).await?; }
        Ok(())
    }
}
