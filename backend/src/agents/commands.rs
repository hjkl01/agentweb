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

fn ensure_workspace_state_dir(
    workspace: &std::path::Path,
    relative: &std::path::Path,
) -> Result<std::path::PathBuf> {
    use std::path::Component;

    let workspace = workspace
        .canonicalize()
        .map_err(|error| anyhow!("cannot resolve Agent workspace: {error}"))?;
    if !workspace.is_dir() {
        bail!("Agent workspace is not a directory");
    }

    let mut current = workspace.clone();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            bail!("invalid relative Agent state directory");
        };
        current.push(name);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let canonical = current
                    .canonicalize()
                    .map_err(|error| anyhow!("cannot resolve Agent state directory: {error}"))?;
                if !canonical.starts_with(&workspace) || !canonical.is_dir() {
                    bail!("Agent state directory must remain inside the workspace");
                }
                current = canonical;
            }
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => bail!("Agent state path is not a directory"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&current)
                    .map_err(|error| anyhow!("cannot create Agent state directory: {error}"))?;
            }
            Err(error) => return Err(error.into()),
        }
        let canonical = current
            .canonicalize()
            .map_err(|error| anyhow!("cannot resolve Agent state directory: {error}"))?;
        if !canonical.starts_with(&workspace) {
            bail!("Agent state directory must remain inside the workspace");
        }
        current = canonical;
    }
    Ok(current)
}

fn is_shared_workspace_root(workspace: &std::path::Path) -> bool {
    workspace == std::path::Path::new("/workspaces")
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
    let mut command = if cfg!(test) {
        // Unit tests use fake executables and intentionally do not require bwrap.
        let mut command = Command::new(&program);
        command.args(args);
        if let Some(dir) = &config.working_directory {
            command.current_dir(dir);
        }
        command
    } else if let Some(dir) = &config.working_directory {
        #[cfg(target_os = "linux")]
        {
            // Fail closed: an Agent with a workspace must not run unrestricted if
            // the Linux namespace sandbox is unavailable. The root filesystem is
            // read-only; only the session workspace is writable.
            let workspace = std::path::Path::new(dir)
                .canonicalize()
                .map_err(|error| anyhow!("cannot resolve Agent workspace: {error}"))?;
            if !workspace.is_dir() {
                bail!("Agent workspace is not a directory");
            }
            // Never allow the shared workspace mount itself as a project:
            // binding it writable would expose every sibling project.
            if is_shared_workspace_root(&workspace) {
                bail!(
                    "Agent workspace must be a project directory, not the shared /workspaces root"
                );
            }
            // The host's Agent home is configurable. Docker sets
            // AGENTWEB_AGENT_HOME=/data/home; native host execution defaults to
            // the process HOME. /data/home below is only the isolated sandbox
            // mount point, not a required host path.
            let agent_home = std::env::var_os("AGENTWEB_AGENT_HOME")
                .or_else(|| std::env::var_os("HOME"))
                .map(std::path::PathBuf::from)
                .ok_or_else(|| anyhow!("AGENTWEB_AGENT_HOME or HOME must point to the Agent home directory"))?
                .canonicalize()
                .map_err(|error| anyhow!("cannot resolve Agent home directory: {error}"))?;
            if !agent_home.is_dir() {
                bail!("Agent home is not a directory: {}", agent_home.display());
            }
            let mut sandbox = Command::new("bwrap");
            sandbox.args([
                "--die-with-parent",
                "--new-session",
                "--unshare-pid",
                "--unshare-ipc",
                "--unshare-uts",
                "--ro-bind",
                "/",
                "/",
                "--dev",
                "/dev",
                "--proc",
                "/proc",
                "--tmpfs",
                "/tmp",
                // Hide unrelated host/container state that is not needed to run
                // an Agent. In particular, do not expose other users' home
                // directories, runtime sockets, package databases, or mount points.
                "--tmpfs",
                "/home",
                "--tmpfs",
                "/root",
                "--tmpfs",
                "/run",
                "--tmpfs",
                "/var",
                "--tmpfs",
                "/mnt",
                "--tmpfs",
                "/sys",
                // Preserve only the required read-only directories under
                // temporary mount aliases before masking /data itself.
                "--dir",
                "/tmp/agentweb-mounts",
                "--dir",
                "/tmp/agentweb-mounts/home",
                "--dir",
                "/tmp/agentweb-mounts/runtimes",
                "--dir",
                "/tmp/agentweb-mounts/workspace",
                // If workspaces are mounted under /workspaces, retain only the
                // selected project and hide sibling projects from the Agent.
                // The alias must be mounted before masking the original parent.
                "--ro-bind",
            ]).arg(&agent_home).args(["/tmp/agentweb-mounts/home"]);
            let runtime_root = std::env::var_os("AGENTWEB_RUNTIME_DIR")
                .map(std::path::PathBuf::from)
                .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
                .filter(|path| path.is_dir())
                .and_then(|path| path.canonicalize().ok());
            if let Some(runtime_root) = &runtime_root {
                sandbox.args(["--ro-bind"]).arg(runtime_root).args([
                    "/tmp/agentweb-mounts/runtimes",
                ]);
            }
            // Hide SQLite and other application data from the Agent.
            sandbox.args([
                "--tmpfs",
                "/data",
                "--dir",
                "/data/home",
                "--dir",
                "/data/runtimes",
                "--ro-bind",
                "/tmp/agentweb-mounts/home",
                "/data/home",
            ]);
            if runtime_root.is_some() {
                sandbox.args([
                    "--ro-bind",
                    "/tmp/agentweb-mounts/runtimes",
                    "/data/runtimes",
                ]);
            }
            sandbox.args(["--bind"]);
            if workspace.starts_with("/workspaces")
                && workspace != std::path::Path::new("/workspaces")
            {
                sandbox.args(["--bind"]);
                sandbox
                    .arg(&workspace)
                    .arg("/tmp/agentweb-mounts/workspace");
                sandbox.args(["--tmpfs", "/workspaces"]);
                let relative = workspace
                    .strip_prefix("/workspaces")
                    .map_err(|error| anyhow!("cannot isolate workspace parent: {error}"))?;
                let mut target = std::path::PathBuf::from("/workspaces");
                for component in relative.components() {
                    target.push(component.as_os_str());
                    sandbox.args(["--dir"]).arg(&target);
                }
                sandbox.args(["--bind", "/tmp/agentweb-mounts/workspace"]);
                sandbox.arg(&workspace);
            } else {
                sandbox.args(["--bind"]).arg(&workspace).arg(&workspace);
            }
            sandbox
                .args(["--chdir"])
                .arg(&workspace)
                .arg("--")
                .arg(&program)
                .args(args);
            sandbox
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (dir, &program, &args);
            bail!("workspace sandbox is currently supported only on Linux; refusing to start an unrestricted Agent");
        }
    } else {
        bail!("Agent execution requires an explicitly selected workspace");
    };
    if let Some(dir) = &config.working_directory {
        command.current_dir(dir);
    }
    // Inside the sandbox the selected host home is mounted at this stable path.
    // Never pass through the host's HOME value, which may be hidden by bwrap.
    command.env("HOME", "/data/home");
    command.env("XDG_CONFIG_HOME", "/data/home/.config");
    command.env("XDG_DATA_HOME", "/data/home/.local/share");
    command.env("XDG_CACHE_HOME", "/data/home/.cache");
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
        "AGENTWEB_RUNTIME_DIR",
        "AGENTWEB_TRUSTED_PROXIES",
    ] {
        command.env_remove(name);
    }
    // Pi supports a dedicated session directory. Keep its writable native
    // conversation state inside the selected workspace instead of its read-only
    // host HOME, while leaving existing Pi credentials/configuration untouched.
    if matches!(kind, ProcessKind::Pi) {
        if let Some(dir) = &config.working_directory {
            let state_dir = ensure_workspace_state_dir(
                std::path::Path::new(dir),
                std::path::Path::new(".agentweb-state/pi-sessions"),
            )?;
            command.env("PI_CODING_AGENT_SESSION_DIR", state_dir);
        }
    }
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

    #[test]
    fn shared_workspaces_root_is_not_a_valid_project_workspace() {
        assert!(is_shared_workspace_root(std::path::Path::new(
            "/workspaces"
        )));
        assert!(!is_shared_workspace_root(std::path::Path::new(
            "/workspaces/project"
        )));
        assert!(!is_shared_workspace_root(std::path::Path::new(
            "/workspaces-other/project"
        )));
    }

    #[test]
    fn pi_command_receives_workspace_session_directory() {
        let workspace = temp_path("pi-command-state");
        std::fs::create_dir_all(&workspace).unwrap();
        let config = AgentConfig {
            id: "pi".into(),
            command: "pi".into(),
            working_directory: Some(workspace.to_string_lossy().into_owned()),
            native_session_id: None,
            fork_from_native_session_id: None,
            runtime_path: None,
            model: None,
        };

        let mut command = build(ProcessKind::Pi, &config, "hello").unwrap();
        let session_dir = command
            .as_std_mut()
            .get_envs()
            .find(|(key, _)| key == std::ffi::OsStr::new("PI_CODING_AGENT_SESSION_DIR"))
            .and_then(|(_, value)| value)
            .expect("Pi session directory environment variable must be set");
        let session_dir = std::path::PathBuf::from(session_dir);
        assert!(session_dir.starts_with(workspace.canonicalize().unwrap()));
        assert!(session_dir.is_dir());

        std::fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn pi_fresh_session_does_not_receive_a_previous_native_session_id() {
        let workspace = temp_path("pi-fresh-session");
        std::fs::create_dir_all(&workspace).unwrap();
        let config = AgentConfig {
            id: "pi".into(),
            command: "pi".into(),
            working_directory: Some(workspace.to_string_lossy().into_owned()),
            native_session_id: None,
            fork_from_native_session_id: None,
            runtime_path: None,
            model: None,
        };
        let command = build(ProcessKind::Pi, &config, "first message").unwrap();
        let args = command
            .as_std()
            .get_args()
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
        let config = AgentConfig {
            id: "pi".into(),
            command: "pi".into(),
            working_directory: Some(workspace.to_string_lossy().into_owned()),
            native_session_id: Some("existing-native-session".into()),
            fork_from_native_session_id: None,
            runtime_path: None,
            model: None,
        };
        let command = build(ProcessKind::Pi, &config, "follow-up").unwrap();
        let args = command
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let session_arg = args
            .iter()
            .position(|arg| arg == "--session")
            .expect("existing Pi session should be resumed");
        assert_eq!(
            args.get(session_arg + 1).map(String::as_str),
            Some("existing-native-session")
        );
        std::fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn pi_fork_uses_fork_flag_without_resuming_source_in_place() {
        let workspace = temp_path("pi-fork-session");
        std::fs::create_dir_all(&workspace).unwrap();
        let config = AgentConfig {
            id: "pi".into(),
            command: "pi".into(),
            working_directory: Some(workspace.to_string_lossy().into_owned()),
            native_session_id: None,
            fork_from_native_session_id: Some("source-native-session".into()),
            runtime_path: None,
            model: None,
        };
        let command = build(ProcessKind::Pi, &config, "continue from this point").unwrap();
        let args = command
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let fork_arg = args
            .iter()
            .position(|arg| arg == "--fork")
            .expect("Pi fork should use the native --fork flag");
        assert_eq!(
            args.get(fork_arg + 1).map(String::as_str),
            Some("source-native-session")
        );
        assert!(!args.iter().any(|arg| arg == "--session"));
        std::fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn pi_session_state_is_created_inside_workspace() {
        let workspace = temp_path("pi-state-workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        let state = ensure_workspace_state_dir(
            &workspace,
            std::path::Path::new(".agentweb-state/pi-sessions"),
        )
        .unwrap();

        assert!(state.starts_with(workspace.canonicalize().unwrap()));
        assert!(state.is_dir());
        std::fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn rejects_invalid_relative_state_paths() {
        let root = temp_path("pi-state-paths");
        let workspace = root.join("workspace");
        let outside = root.join("outside");
        std::fs::create_dir_all(&workspace).unwrap();

        assert!(
            ensure_workspace_state_dir(&workspace, std::path::Path::new("../outside")).is_err()
        );
        assert!(
            ensure_workspace_state_dir(&workspace, std::path::Path::new("/tmp/outside")).is_err()
        );
        assert!(!outside.exists());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_pi_session_state_symlink_escape() {
        use std::os::unix::fs::symlink;

        let workspace = temp_path("pi-state-workspace");
        let outside = temp_path("pi-state-outside");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        symlink(&outside, workspace.join(".agentweb-state")).unwrap();

        let result = ensure_workspace_state_dir(
            &workspace,
            std::path::Path::new(".agentweb-state/pi-sessions"),
        );

        assert!(result.is_err());
        assert!(!outside.join("pi-sessions").exists());
        std::fs::remove_dir_all(workspace).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }
}
