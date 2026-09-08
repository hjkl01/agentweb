use super::adapter::AgentConfig;
use anyhow::{anyhow, Result};
use tokio::process::Command;

#[derive(Clone, Copy)]
pub enum ProcessKind { Codex, OpenCode, Pi, Generic }

fn parts(command: &str) -> Result<(String, Vec<String>)> {
    let mut parts = command.split_whitespace();
    let program = parts.next().ok_or_else(|| anyhow!("empty agent command"))?.to_owned();
    Ok((program, parts.map(str::to_owned).collect()))
}

pub fn build(kind: ProcessKind, config: &AgentConfig, message: &str) -> Result<Command> {
    let (program, base) = parts(&config.command)?;
    let mut command = Command::new(program);
    let mut args = base;
    match kind {
        ProcessKind::Codex => {
            args = if let Some(id) = &config.native_session_id {
                vec!["exec".into(), "resume".into(), id.clone(), "--json".into()]
            } else {
                vec!["exec".into(), "--json".into()]
            };
            if let Some(model) = &config.model { args.extend(["--model".into(), model.clone()]); }
            args.push(message.into());
        }
        ProcessKind::Pi => {
            args.extend(["--mode".into(), "json".into()]);
            if let Some(model) = &config.model { args.extend(["--model".into(), model.clone()]); }
            if let Some(id) = &config.native_session_id { args.extend(["--session".into(), id.clone()]); }
            args.extend(["-p".into(), message.into()]);
        }
        ProcessKind::OpenCode => {
            args.push("run".into());
            if let Some(model) = &config.model { args.extend(["--model".into(), model.clone()]); }
            if let Some(id) = &config.native_session_id { args.extend(["--session".into(), id.clone()]); }
            args.extend([message.into(), "--format".into(), "json".into()]);
        }
        ProcessKind::Generic => args.push(message.into()),
    }
    command.args(args);
    if let Some(dir) = &config.working_directory { command.current_dir(dir); }
    if let Some(runtime) = &config.runtime_path {
        let path = std::env::var("PATH").unwrap_or_default();
        command.env("PATH", format!("{runtime}:{path}"));
    }
    command.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    Ok(command)
}