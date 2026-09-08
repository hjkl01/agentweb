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
        return Err(anyhow::anyhow!("model discovery command failed"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn parse_model_lines(output: &str, source: &str) -> Vec<AgentModel> {
    let mut result = Vec::new();
    for raw in output.lines() {
        let line = raw.replace('\u{1b}', "");
        let line = line.trim().trim_matches(|c: char| c == '[' || c == ']' || c == '|');
        if line.is_empty() || line.contains("Available models") || line.starts_with("Provider") {
            continue;
        }
        let candidate = line
            .split_whitespace()
            .find(|token| token.contains('/') && !token.starts_with('-'))
            .unwrap_or(line.split_whitespace().next().unwrap_or(""))
            .trim_matches('|')
            .trim();
        if candidate.is_empty() || candidate.len() > 200 || candidate.contains(':') && candidate.ends_with(':') {
            continue;
        }
        let provider = candidate.split('/').next().map(str::to_owned);
        if result.iter().any(|m: &AgentModel| m.id == candidate) {
            continue;
        }
        result.push(AgentModel {
            id: candidate.to_owned(),
            name: candidate.to_owned(),
            provider,
            source: source.to_owned(),
        });
    }
    result
}

fn codex_config_paths() -> Vec<PathBuf> {
    let home = env::var_os("HOME").map(PathBuf::from);
    let root = env::var_os("CODEX_HOME").map(PathBuf::from).or(home.map(|h| h.join(".codex")));
    root.into_iter().map(|p| p.join("config.toml")).collect()
}

fn codex_config_models() -> Vec<AgentModel> {
    let mut result = Vec::new();
    for path in codex_config_paths() {
        let Ok(text) = fs::read_to_string(path) else { continue };
        for line in text.lines() {
            let trimmed = line.trim();
            if !trimmed.starts_with("model") || !trimmed.contains('=') {
                continue;
            }
            let Some(value) = trimmed.split_once('=').map(|(_, v)| v.trim()) else { continue };
            let value = value.trim_matches('"').trim_matches('\'');
            if value.is_empty() || result.iter().any(|m: &AgentModel| m.id == value) {
                continue;
            }
            result.push(AgentModel {
                id: value.to_owned(),
                name: value.to_owned(),
                provider: Some("openai".into()),
                source: "codex config.toml".into(),
            });
        }
    }
    result
}

pub async fn discover(kind: &str, config: &AgentConfig) -> Result<Vec<AgentModel>> {
    let models = match kind {
        "opencode" => parse_model_lines(&command_output(config, &["models"]).await?, "opencode models"),
        "pi" => parse_model_lines(&command_output(config, &["--list-models"]).await?, "pi --list-models"),
        "codex" => codex_config_models(),
        _ => Vec::new(),
    };
    Ok(models)
}
