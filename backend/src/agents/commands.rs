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
            args = vec!["--output-format".into(), "stream-json".into(), "--verbose".into()];
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
            args = vec!["--output-format".into(), "stream-json".into(), "--include-partial-messages".into()];
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
            if let Some(id) = &config.native_session_id {
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
    let mut command = if let Some(dir) = &config.working_directory {
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
            let mut sandbox = Command::new("bwrap");
            sandbox.args([
                "--die-with-parent",
                "--new-session",
                "--ro-bind", "/", "/",
                "--dev-bind", "/dev", "/dev",
                "--proc", "/proc",
                "--tmpfs", "/tmp",
                "--bind",
            ]);
            sandbox.arg(&workspace).arg(&workspace);
            sandbox.args(["--chdir"]).arg(&workspace).arg("--").arg(&program).args(args);
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
    if let Some(runtime) = &config.runtime_path {
        let path = std::env::var("PATH").unwrap_or_default();
        command.env("PATH", format!("{runtime}:{path}"));
    }
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    Ok(command)
}
