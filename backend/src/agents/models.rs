use super::adapter::AgentConfig;
use anyhow::Result;
use serde::Serialize;
use std::{env, fs, path::PathBuf, process::Stdio};
use tokio::process::Command;

#[derive(Debug, Clone, Serialize)]
pub struct AgentModel {
    pub id: String,
    pub name: String,
    pub provider: Option<String>,
    pub source: String,
}

fn command_parts(command: &str) -> Result<(String, Vec<String>)> {
    let mut parts = command.split_whitespace();
    let program = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("empty agent command"))?
        .to_owned();
    Ok((program, parts.map(str::to_owned).collect()))
}

async fn command_output(config: &AgentConfig, extra: &[&str]) -> Result<String> {
    let (program, base) = command_parts(&config.command)?;
    let mut command = Command::new(program);
    command.args(base).args(extra).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(dir) = &config.working_directory {
        command.current_dir(dir);
    }
    if let Some(runtime) = &config.runtime_path {
        command.env("PATH", format!("{}:{}", runtime, env::var("PATH").unwrap_or_default()));
    }
    let output = command.output().await?;
    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "model discovery command failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Parse the human-readable model table emitted by `pi --list-models`.
///
/// Pi intentionally owns its model registry and authentication state. We ask
/// Pi itself instead of reading provider credentials or duplicating its registry.
fn parse_pi_models(output: &str) -> Vec<AgentModel> {
    let mut result = Vec::new();
    for raw in output.lines() {
        let line = raw.replace('\u{1b}', "");
        let line = line.trim();
        if line.is_empty() || line.starts_with("Provider") || line.starts_with("Model") {
            continue;
        }

        // Typical rows are: provider  model  context ...
        let mut columns = line.split_whitespace();
        let Some(provider) = columns.next() else { continue };
        let Some(model) = columns.next() else { continue };
        if provider.starts_with('-') || model.starts_with('-') {
            continue;
        }

        let id = if model.contains('/') {
            model.to_owned()
        } else {
            format!("{provider}/{model}")
        };
        if result.iter().any(|m: &AgentModel| m.id == id) {
            continue;
        }
        result.push(AgentModel {
            id: id.clone(),
            name: model.to_owned(),
            provider: Some(provider.to_owned()),
            source: "pi --list-models".into(),
        });
    }
    result
}

/// Codex reads models from CODEX_HOME/config.toml (or ~/.codex/config.toml).
///
/// Do not assume the model is only at the top level: Codex configurations can
/// contain profiles/sections, so collect every `model = "..."` entry while
/// deliberately ignoring unrelated TOML values.
fn codex_config_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(path) = env::var_os("CODEX_HOME") {
        paths.push(PathBuf::from(path).join("config.toml"));
    }
    if let Some(home) = env::var_os("HOME") {
        paths.push(PathBuf::from(home).join(".codex/config.toml"));
    }
    paths.sort();
    paths.dedup();
    paths
}

fn codex_config_models() -> Vec<AgentModel> {
    let mut result = Vec::new();
    for path in codex_config_paths() {
        let Ok(text) = fs::read_to_string(&path) else { continue };
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') || !trimmed.starts_with("model") || !trimmed.contains('=') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else { continue };
            if key.trim() != "model" {
                continue;
            }
            let value = value
                .split('#')
                .next()
                .unwrap_or(value)
                .trim()
                .trim_matches('"')
                .trim_matches('\'');
            if value.is_empty() || result.iter().any(|m: &AgentModel| m.id == value) {
                continue;
            }
            result.push(AgentModel {
                id: value.to_owned(),
                name: value.to_owned(),
                provider: Some("openai".into()),
                source: format!("{}", path.display()),
            });
        }
    }
    result
}

pub async fn discover(kind: &str, config: &AgentConfig) -> Result<Vec<AgentModel>> {
    match kind {
        // These are the first two adapters with explicit model discovery. Other
        // agents return an empty list until their own configuration surface is
        // implemented, rather than pretending their models are compatible.
        "codex" => Ok(codex_config_models()),
        "pi" => Ok(parse_pi_models(&command_output(config, &["--list-models"]).await?)),
        _ => Ok(Vec::new()),
    }
}
