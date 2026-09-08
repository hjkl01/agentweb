#[derive(Debug, Clone)]
pub struct AgentDefinition {
    pub id: &'static str,
    #[allow(dead_code)]
    pub name: &'static str,
    pub kind: &'static str,
    #[allow(dead_code)]
    pub command: &'static str,
}

pub const BUILT_IN_AGENTS: &[AgentDefinition] = &[
    AgentDefinition { id: "codex", name: "Codex", kind: "codex", command: "codex" },
    AgentDefinition { id: "claude-code", name: "Claude Code", kind: "claude-code", command: "claude" },
    AgentDefinition { id: "pi", name: "Pi", kind: "pi", command: "pi" },
    AgentDefinition { id: "opencode", name: "OpenCode", kind: "opencode", command: "opencode" },
    AgentDefinition { id: "openclaw", name: "OpenClaw", kind: "openclaw", command: "openclaw" },
];
