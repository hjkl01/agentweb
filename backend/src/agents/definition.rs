use crate::agents::AgentConfig;

#[derive(Debug, Clone)]
pub struct AgentDefinition {
    pub id: &'static str,
    pub name: &'static str,
    pub kind: &'static str,
    pub command: &'static str,
}

pub const BUILT_IN_AGENTS: &[AgentDefinition] = &[
    AgentDefinition { id: "codex", name: "Codex", kind: "codex", command: "codex" },
    AgentDefinition { id: "claude-code", name: "Claude Code", kind: "claude-code", command: "claude" },
    AgentDefinition { id: "pi", name: "Pi", kind: "pi", command: "pi" },
    AgentDefinition { id: "opencode", name: "OpenCode", kind: "opencode", command: "opencode" },
    AgentDefinition { id: "openclaw", name: "OpenClaw", kind: "openclaw", command: "openclaw" },
];

pub fn config_for(id: &str) -> Option<AgentConfig> {
    BUILT_IN_AGENTS.iter().find(|agent| agent.id == id).map(|agent| AgentConfig {
        id: agent.id.to_owned(),
        command: agent.command.to_owned(),
        working_directory: None,
        native_session_id: None,
        runtime_path: None,
        model: None,
    })
}
