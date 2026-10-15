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
pub mod pi_transcript;
pub mod process;
pub mod session;

pub use adapter::{AgentAdapter, AgentConfig};
use anyhow::Result;
use codex::CodexAdapter;
use commands::ProcessKind;
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
            "claude-code" | "claude" => Arc::new(GenericAdapter::with_kind(ProcessKind::Claude)),
            "gemini-cli" | "gemini" => Arc::new(GenericAdapter::with_kind(ProcessKind::Gemini)),
            "qwen-code" | "qwen" => Arc::new(GenericAdapter::with_kind(ProcessKind::Qwen)),
            "opencode" => Arc::new(OpenCodeAdapter::new()),
            "pi" => Arc::new(PiAdapter::new()),
            _ => Arc::new(GenericAdapter::new()),
        };
        map.insert(kind.to_owned(), adapter.clone());
        adapter
    }

    pub async fn interrupt(
        &self,
        kind: &str,
        session_id: &str,
        execution_id: &str,
    ) -> Result<()> {
        // An execution token belongs to one adapter only. Broadcasting a
        // pre-start cancellation marker to every adapter would leak markers and
        // could affect unrelated future runs.
        self.adapter(kind)
            .await
            .interrupt(session_id, execution_id)
            .await
    }
}


#[cfg(test)]
mod tests {
    use super::{AgentAdapter, AgentConfig, AgentManager};
    use crate::{agents::{adapter::AgentRunResult, models::AgentModel}, events::EventBus};
    use anyhow::{bail, Result};
    use async_trait::async_trait;
    use std::sync::{atomic::{AtomicUsize, Ordering}, Arc};

    struct MockAdapter {
        interrupt_calls: Arc<AtomicUsize>,
        should_fail: bool,
    }

    #[async_trait]
    impl AgentAdapter for MockAdapter {
        async fn send_message(
            &self,
            _config: &AgentConfig,
            _session_id: &str,
            _execution_id: &str,
            _message: &str,
            _events: &EventBus,
        ) -> Result<AgentRunResult> {
            bail!("send_message is not used in this test")
        }

        async fn interrupt(&self, _session_id: &str, _execution_id: &str) -> Result<()> {
            self.interrupt_calls.fetch_add(1, Ordering::SeqCst);
            if self.should_fail {
                bail!("mock interrupt failure")
            }
            Ok(())
        }

        async fn list_models(&self, _config: &AgentConfig) -> Result<Vec<AgentModel>> {
            Ok(Vec::new())
        }
    }

    #[tokio::test]
    async fn interrupt_targets_only_the_selected_adapter() {
        let manager = AgentManager::new();
        let failed_calls = Arc::new(AtomicUsize::new(0));
        let successful_calls = Arc::new(AtomicUsize::new(0));

        {
            let mut adapters = manager.adapters.lock().await;
            adapters.insert(
                "failing".to_owned(),
                Arc::new(MockAdapter {
                    interrupt_calls: failed_calls.clone(),
                    should_fail: true,
                }),
            );
            adapters.insert(
                "successful".to_owned(),
                Arc::new(MockAdapter {
                    interrupt_calls: successful_calls.clone(),
                    should_fail: false,
                }),
            );
        }

        let result = manager.interrupt("failing", "test-session", "test-execution").await;

        assert!(result.is_err(), "the first adapter error must be returned");
        assert_eq!(failed_calls.load(Ordering::SeqCst), 1);
        assert_eq!(successful_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn interrupt_succeeds_when_all_adapters_succeed() {
        let manager = AgentManager::new();
        let calls = Arc::new(AtomicUsize::new(0));
        manager.adapters.lock().await.insert(
            "successful".to_owned(),
            Arc::new(MockAdapter {
                interrupt_calls: calls.clone(),
                should_fail: false,
            }),
        );

        assert!(manager.interrupt("successful", "test-session", "test-execution").await.is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
