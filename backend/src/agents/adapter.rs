use crate::events::EventBus;
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{error::Error, fmt};

use super::models::AgentModel;

#[derive(Debug, Clone)]
pub struct AgentRunError {
    pub interrupted: bool,
    pub native_session_id: Option<String>,
    pub native_session_missing: bool,
    message: String,
}

impl AgentRunError {
    pub fn interrupted(native_session_id: Option<String>) -> Self {
        Self {
            interrupted: true,
            native_session_id,
            native_session_missing: false,
            message: "agent process interrupted".into(),
        }
    }

    pub fn failed(message: impl Into<String>, native_session_id: Option<String>) -> Self {
        Self::failed_with_session_state(message, native_session_id, false)
    }

    pub fn missing_session(message: impl Into<String>, native_session_id: Option<String>) -> Self {
        Self::failed_with_session_state(message, native_session_id, true)
    }

    fn failed_with_session_state(
        message: impl Into<String>,
        native_session_id: Option<String>,
        native_session_missing: bool,
    ) -> Self {
        Self {
            interrupted: false,
            native_session_id,
            native_session_missing,
            message: message.into(),
        }
    }
}

impl fmt::Display for AgentRunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for AgentRunError {}

#[derive(Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub id: String,
    pub command: String,
    pub working_directory: Option<String>,
    pub native_session_id: Option<String>,
    #[serde(default)]
    pub fork_from_native_session_id: Option<String>,
    pub runtime_path: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub thinking_level: Option<String>,
    #[serde(default)]
    pub tool_preset: Option<String>,
}

/// Reasoning levels accepted by the Pi CLI's `--thinking` flag, from least to
/// most effort. Pi also accepts `off`.
pub const THINKING_LEVELS: &[&str] = &["off", "minimal", "low", "medium", "high", "xhigh", "max"];

/// Accept a known level verbatim; unknown values are rejected so a typo cannot
/// silently change what the CLI receives.
pub fn normalize_thinking_level(level: &str) -> Option<String> {
    let level = level.trim().to_ascii_lowercase();
    THINKING_LEVELS.contains(&level.as_str()).then_some(level)
}

/// Session-level tool presets. Pi resolves the `--tools` entries against its own tool set
/// (`read`, `bash`, `powershell`, `edit`, `write`, `grep`, `find`, `ls`, plus extension and
/// MCP tools), so a preset only removes capabilities instead of replacing `defaultTools`.
pub const TOOL_PRESETS: &[&str] = &["default", "readonly", "nobash", "notools"];

/// Accept a known preset verbatim; unknown values are rejected so a typo cannot silently
/// change what the CLI receives.
pub fn normalize_tool_preset(preset: &str) -> Option<String> {
    let preset = preset.trim().to_ascii_lowercase();
    TOOL_PRESETS.contains(&preset.as_str()).then_some(preset)
}

/// Extra arguments a preset adds to the Pi command line; `default` adds nothing.
pub fn tool_preset_args(preset: &str) -> &'static [&'static str] {
    match preset {
        "readonly" => &["--tools", "-bash,-edit,-write"],
        "nobash" => &["--tools", "-bash"],
        "notools" => &["--no-tools"],
        _ => &[],
    }
}

#[derive(Debug, Clone, Default)]
pub struct AgentRunResult {
    pub native_session_id: Option<String>,
    pub assistant_text: String,
}

#[async_trait]
pub trait AgentAdapter: Send + Sync {
    async fn send_message(
        &self,
        config: &AgentConfig,
        session_id: &str,
        execution_id: &str,
        message: &str,
        events: &EventBus,
    ) -> Result<AgentRunResult>;

    async fn interrupt(&self, session_id: &str, execution_id: &str) -> Result<()>;

    async fn list_models(&self, config: &AgentConfig) -> Result<Vec<AgentModel>> {
        crate::agents::models::discover(&config.id, config).await
    }
}

#[cfg(test)]
mod thinking_level_tests {
    use super::*;

    #[test]
    fn normalizes_known_levels_and_rejects_unknown() {
        assert_eq!(normalize_thinking_level(" High ").as_deref(), Some("high"));
        assert_eq!(normalize_thinking_level("xhigh").as_deref(), Some("xhigh"));
        for rejected in ["", "highest", "true", "medium reasoning"] {
            assert_eq!(normalize_thinking_level(rejected), None, "{rejected:?}");
        }
    }
}
#[cfg(test)]
mod tool_preset_tests {
    use super::{normalize_tool_preset, tool_preset_args};

    #[test]
    fn normalizes_known_presets_and_rejects_unknown() {
        assert_eq!(
            normalize_tool_preset(" ReadOnly ").as_deref(),
            Some("readonly")
        );
        assert_eq!(normalize_tool_preset("notools").as_deref(), Some("notools"));
        for rejected in ["", "all", "read-only", "none", "defaulttools"] {
            assert_eq!(normalize_tool_preset(rejected), None, "{rejected:?}");
        }
    }

    #[test]
    fn every_preset_maps_to_argv_or_nothing() {
        assert!(tool_preset_args("default").is_empty());
        assert_eq!(
            tool_preset_args("readonly"),
            ["--tools", "-bash,-edit,-write"]
        );
        // `-name` entries keep the user's own `defaultTools` selection intact.
        assert!(tool_preset_args("nobash").iter().all(|arg| !arg.is_empty()));
        assert_eq!(tool_preset_args("notools"), ["--no-tools"]);
    }
}
