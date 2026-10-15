use super::adapter::AgentConfig;
use anyhow::{anyhow, bail, Result};
use tokio::process::Command;

#[derive(Clone, Copy)]
pub enum ProcessKind {
    Codex,
    Claude,
    Gemini,
    Qwen,
    OpenCode,
    Pi,
    Generic,
}

fn parts(command: &str) -> Result<(String, Vec<String>)> {
    let mut parts = command.split_whitespace();
    let program = parts
        .next()
        .ok_or_else(|| anyhow!("empty agent command"))?
        .to_owned();
    Ok((program, parts.map(str::to_owned).collect()))
}

pub fn build(kind: ProcessKind, config: &AgentConfig, message: &str) -> Result<Command> {
    let (program, base) = parts(&config.command)?;
    let mut args = base;
    match kind {
        ProcessKind::Codex => {
            args = if let Some(id) = &config.native_session_id {
                vec!["exec".into(), "resume".into(), id.clone(), "--json".into()]
            } else {
                vec!["exec".into(), "--json".into()]
            };
            if let Some(model) = &config.model {
                args.extend(["--model".into(), model.clone()]);
            }
            args.push(message.into());
        }
        ProcessKind::Claude => {
            args = vec![
                "--output-format".into(),
                "stream-json".into(),
                "--verbose".into(),
            ];
            if let Some(id) = &config.native_session_id {
                args.extend(["--resume".into(), id.clone()]);
            }
            if let Some(model) = &config.model {
                args.extend(["--model".into(), model.clone()]);
            }
            args.extend(["-p".into(), message.into()]);
        }
        ProcessKind::Gemini => {
            args = vec!["--output-format".into(), "stream-json".into()];
            if let Some(id) = &config.native_session_id {
                args.extend(["--resume".into(), id.clone()]);
            }
            if let Some(model) = &config.model {
                args.extend(["--model".into(), model.clone()]);
            }
            args.extend(["--prompt".into(), message.into()]);
        }
        ProcessKind::Qwen => {
            args = vec![
                "--output-format".into(),
                "stream-json".into(),
                "--include-partial-messages".into(),
            ];
            if let Some(id) = &config.native_session_id {
                args.extend(["--resume".into(), id.clone()]);
            }
            if let Some(model) = &config.model {
                args.extend(["--model".into(), model.clone()]);
            }
            args.extend(["-p".into(), message.into()]);
        }
        ProcessKind::Pi => {
            args.extend(["--mode".into(), "json".into()]);
            if let Some(model) = &config.model {
                args.extend(["--model".into(), model.clone()]);
            }
            if let Some(id) = &config.fork_from_native_session_id {
                args.extend(["--fork".into(), id.clone()]);
            } else if let Some(id) = &config.native_session_id {
                args.extend(["--session".into(), id.clone()]);
            }
            args.extend(["-p".into(), message.into()]);
        }
        ProcessKind::OpenCode => {
            args.push("run".into());
            if let Some(model) = &config.model {
                args.extend(["--model".into(), model.clone()]);
            }
            if let Some(id) = &config.native_session_id {
                args.extend(["--session".into(), id.clone()]);
            }
            args.extend([message.into(), "--format".into(), "json".into()]);
        }
        ProcessKind::Generic => args.push(message.into()),
    }
    let mut command = Command::new(&program);
    command.args(args);
    let agent_home = std::env::var_os("AGENTWEB_AGENT_HOME")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from)
        .ok_or_else(|| anyhow!("AGENTWEB_AGENT_HOME or HOME must point to the Agent home directory"))?
        .canonicalize()
        .map_err(|error| anyhow!("cannot resolve Agent home directory: {error}"))?;
    if !agent_home.is_dir() {
        bail!("Agent home is not a directory: {}", agent_home.display());
    }
    if let Some(dir) = &config.working_directory {
        let workspace = std::path::Path::new(dir)
            .canonicalize()
            .map_err(|error| anyhow!("cannot resolve Agent workspace: {error}"))?;
        if !workspace.is_dir() {
            bail!("Agent workspace is not a directory");
        }
        command.current_dir(workspace);
    } else {
        bail!("Agent execution requires an explicitly selected workspace");
    }
    // Agents run directly on the host: use the real home and native XDG paths.
    // No bwrap, namespace isolation, filesystem overlay, or bind mounts.
    command.env("HOME", &agent_home);
    command.env("XDG_CONFIG_HOME", agent_home.join(".config"));
    command.env("XDG_DATA_HOME", agent_home.join(".local/share"));
    command.env("XDG_CACHE_HOME", agent_home.join(".cache"));
    // Do not leak AgentWeb's internal service configuration into third-party
    // Agent processes. Provider credentials and proxy settings remain inherited.
    for name in [
        "DATABASE_URL",
        "AGENTWEB_BIND_ADDR",
        "AGENTWEB_EVENT_LISTEN_ADDR",
        "AGENTWEB_EVENT_PEERS",
        "AGENTWEB_WORKERS",
        "AGENTWEB_FRONTEND_DIR",
        "AGENTWEB_WORKSPACE_DIR",
        "AGENTWEB_AGENT_HOME",
        "AGENTWEB_RUNTIME_DIR",
        "AGENTWEB_TRUSTED_PROXIES",
    ] {
        command.env_remove(name);
    }
    // Do not override PI_CODING_AGENT_SESSION_DIR: Pi selects its native session store under HOME.
    if let Some(runtime) = &config.runtime_path {
        let path = std::env::var("PATH").unwrap_or_default();
        command.env("PATH", format!("{runtime}:{path}"));
    }
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    Ok(command)
}

#[cfg(test)]
mod workspace_state_tests {
    use super::*;
    use uuid::Uuid;

    fn temp_path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("agentweb-{label}-{}", Uuid::new_v4()))
    }

    fn config(workspace: &std::path::Path) -> AgentConfig {
        AgentConfig {
            id: "pi".into(),
            command: "pi".into(),
            working_directory: Some(workspace.to_string_lossy().into_owned()),
            native_session_id: None,
            fork_from_native_session_id: None,
            runtime_path: None,
            model: None,
        }
    }

    #[test]
    fn pi_command_uses_native_home_without_sandbox_or_session_override() {
        let workspace = temp_path("pi-native-home");
        std::fs::create_dir_all(&workspace).unwrap();
        let command = build(ProcessKind::Pi, &config(&workspace), "hello").unwrap();
        let args = command.as_std().get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(!args.iter().any(|arg| arg == "bwrap"));
        assert!(!command.as_std().get_envs()
            .any(|(key, _)| key == std::ffi::OsStr::new("PI_CODING_AGENT_SESSION_DIR")));
        let canonical_workspace = workspace.canonicalize().unwrap();
        assert_eq!(command.as_std().get_current_dir(), Some(canonical_workspace.as_path()));
        std::fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn pi_fresh_session_does_not_receive_a_previous_native_session_id() {
        let workspace = temp_path("pi-fresh-session");
        std::fs::create_dir_all(&workspace).unwrap();
        let command = build(ProcessKind::Pi, &config(&workspace), "first message").unwrap();
        let args = command.as_std().get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(!args.iter().any(|arg| arg == "--session"));
        assert!(!args.iter().any(|arg| arg == "old-native-session"));
        std::fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn pi_existing_session_passes_its_native_session_id() {
        let workspace = temp_path("pi-resumed-session");
        std::fs::create_dir_all(&workspace).unwrap();
        let mut config = config(&workspace);
        config.native_session_id = Some("existing-native-session".into());
        let command = build(ProcessKind::Pi, &config, "follow-up").unwrap();
        let args = command.as_std().get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let session_arg = args.iter().position(|arg| arg == "--session")
            .expect("existing Pi session should be resumed");
        assert_eq!(args.get(session_arg + 1).map(String::as_str), Some("existing-native-session"));
        std::fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn pi_fork_uses_fork_flag_without_resuming_source_in_place() {
        let workspace = temp_path("pi-fork-session");
        std::fs::create_dir_all(&workspace).unwrap();
        let mut config = config(&workspace);
        config.fork_from_native_session_id = Some("source-native-session".into());
        let command = build(ProcessKind::Pi, &config, "continue from this point").unwrap();
        let args = command.as_std().get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let fork_arg = args.iter().position(|arg| arg == "--fork")
            .expect("Pi fork should use the native --fork flag");
        assert_eq!(args.get(fork_arg + 1).map(String::as_str), Some("source-native-session"));
        assert!(!args.iter().any(|arg| arg == "--session"));
        std::fs::remove_dir_all(workspace).unwrap();
    }
}
