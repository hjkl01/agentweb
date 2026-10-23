use super::adapter::AgentConfig;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{env, fs, path::PathBuf, process::Stdio};
use tokio::process::Command;

#[derive(Debug, Clone, Serialize)]
pub struct AgentModel {
    pub id: String,
    pub name: String,
    pub provider: Option<String>,
    pub source: String,
    /// `pi --list-models` 的 context 列，例如 `128K` → 128000。缺省表示未知。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
}

/// Runs an Agent CLI subcommand. `timeout` bounds long introspection commands such as
/// `pi mcp list`, which connects to every configured server.
pub(super) async fn run_agent_command(
    config: &AgentConfig,
    extra: &[&str],
    timeout: Option<std::time::Duration>,
) -> Result<(bool, String, String)> {
    let (program, base) = super::commands::command_parts(&config.command)?;
    let mut command = Command::new(program);
    command
        .args(base)
        .args(extra)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(dir) = &config.working_directory {
        command.current_dir(dir);
    }
    // Introspection must see the same home, and therefore the same Agent configuration,
    // as a real run (see `commands::build`).
    super::commands::apply_agent_environment(config, &mut command, None);
    let output = match timeout {
        Some(limit) => tokio::time::timeout(limit, command.output())
            .await
            .map_err(|_| anyhow::anyhow!("agent command timed out after {limit:?}"))??,
        None => command.output().await?,
    };
    Ok((
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    ))
}

async fn command_output_status(
    config: &AgentConfig,
    extra: &[&str],
) -> Result<(bool, String, String)> {
    run_agent_command(config, extra, None).await
}

async fn command_output(config: &AgentConfig, extra: &[&str]) -> Result<String> {
    let (success, stdout, stderr) = command_output_status(config, extra).await?;
    if !success {
        return Err(anyhow::anyhow!(
            "model discovery command failed: {}",
            stderr.trim()
        ));
    }
    Ok(stdout)
}

fn clean_cli_line(line: &str) -> String {
    let mut result = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for next in chars.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            continue;
        }
        result.push(ch);
    }
    result.trim().to_owned()
}

/// context 列的写法：`128K` / `1M` / `200000`。
fn parse_context_window(raw: &str) -> Option<u64> {
    let text = raw.trim();
    let (number, factor) = match text.chars().last()? {
        'K' | 'k' => (&text[..text.len() - 1], 1_000f64),
        'M' | 'm' => (&text[..text.len() - 1], 1_000_000f64),
        _ => (text, 1f64),
    };
    let value: f64 = number.trim().parse().ok()?;
    (value > 0.0).then(|| (value * factor).round() as u64)
}

fn parse_pi_models(output: &str) -> Vec<AgentModel> {
    let mut result = Vec::new();
    for raw in output.lines() {
        let line = clean_cli_line(raw);
        if line.is_empty()
            || line.starts_with("Provider")
            || line.starts_with("Model")
            || line.starts_with('─')
        {
            continue;
        }
        let mut columns = line.split_whitespace();
        let Some(provider) = columns.next() else {
            continue;
        };
        let Some(model) = columns.next() else {
            continue;
        };
        let context_window = columns.next().and_then(parse_context_window);
        if provider.starts_with('-')
            || model.starts_with('-')
            || provider.eq_ignore_ascii_case("provider")
        {
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
            id,
            name: model.to_owned(),
            provider: Some(provider.to_owned()),
            source: "pi --list-models".into(),
            context_window,
        });
    }
    result
}

fn pi_config_paths() -> Vec<PathBuf> {
    vec![super::config::pi_agent_dir().join("models.json")]
}

fn strip_jsonc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut string = false;
    let mut escaped = false;
    while let Some(ch) = chars.next() {
        if string {
            out.push(ch);
            if escaped {
                escaped = false
            } else if ch == '\\' {
                escaped = true
            } else if ch == '"' {
                string = false
            }
            continue;
        }
        if ch == '"' {
            string = true;
            out.push(ch);
            continue;
        }
        if ch == '/' && chars.peek() == Some(&'/') {
            chars.next();
            for next in chars.by_ref() {
                if next == '\n' {
                    out.push('\n');
                    break;
                }
            }
            continue;
        }
        if ch == '/' && chars.peek() == Some(&'*') {
            chars.next();
            while let Some(next) = chars.next() {
                if next == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    break;
                }
            }
            continue;
        }
        out.push(ch);
    }
    let chars: Vec<char> = out.chars().collect();
    let mut result = String::with_capacity(out.len());
    let mut in_string = false;
    let mut escaped = false;
    for i in 0..chars.len() {
        let ch = chars[i];
        if ch == '"' && !escaped {
            in_string = !in_string;
        }
        if ch == ',' && !in_string {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if j < chars.len() && matches!(chars[j], '}' | ']') {
                continue;
            }
        }
        result.push(ch);
        escaped = ch == '\\' && !escaped;
        if ch != '\\' {
            escaped = false;
        }
    }
    result
}

#[derive(Debug, Deserialize)]
struct PiModelsFile {
    providers: Option<std::collections::HashMap<String, PiProvider>>,
}
#[derive(Debug, Deserialize)]
struct PiProvider {
    models: Option<Vec<PiModel>>,
}
#[derive(Debug, Deserialize)]
struct PiModel {
    id: Option<String>,
    name: Option<String>,
}

fn pi_config_models() -> Vec<AgentModel> {
    let mut result = Vec::new();
    for path in pi_config_paths() {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(config) = serde_json::from_str::<PiModelsFile>(&strip_jsonc(&text)) else {
            continue;
        };
        for (provider, entry) in config.providers.unwrap_or_default() {
            for model in entry.models.unwrap_or_default() {
                let Some(id) = model.id.filter(|v| !v.trim().is_empty()) else {
                    continue;
                };
                let full_id = format!("{provider}/{id}");
                if result.iter().any(|m: &AgentModel| m.id == full_id) {
                    continue;
                }
                result.push(AgentModel {
                    id: full_id,
                    name: model.name.unwrap_or_else(|| id.clone()),
                    provider: Some(provider.clone()),
                    source: path.to_string_lossy().into_owned(),
                    context_window: None,
                });
            }
        }
    }
    result
}
fn merge_models(mut configured: Vec<AgentModel>, discovered: Vec<AgentModel>) -> Vec<AgentModel> {
    for model in discovered {
        match configured.iter_mut().find(|item| item.id == model.id) {
            // 配置里的条目不带窗口信息，用 `pi --list-models` 的补上。
            Some(existing) => {
                existing.context_window = existing.context_window.or(model.context_window);
            }
            None => configured.push(model),
        }
    }
    configured
}

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
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') || !trimmed.starts_with("model") || !trimmed.contains('=') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
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
                source: path.to_string_lossy().into_owned(),
                context_window: None,
            });
        }
    }
    result
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderAuth {
    pub provider: String,
    pub status: String,
    pub auth_type: Option<String>,
    pub reason: Option<String>,
}

#[derive(Deserialize)]
struct PiAuthReport {
    status: Option<String>,
    #[serde(rename = "authType")]
    auth_type: Option<String>,
    reason: Option<String>,
}

/// CLI identifiers are passed as a separate argv entry, so this only guards against
/// a value that Pi would interpret as another flag.
pub fn valid_cli_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && !value.starts_with('-')
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
}

fn parse_auth_report(provider: &str, text: &str) -> Result<ProviderAuth> {
    let text = text.trim();
    if text.is_empty() {
        return Err(anyhow::anyhow!("credential check produced no output"));
    }
    if let Ok(report) = serde_json::from_str::<PiAuthReport>(text) {
        return Ok(ProviderAuth {
            provider: provider.to_owned(),
            status: report.status.unwrap_or_else(|| "unknown".into()),
            auth_type: report.auth_type,
            reason: report.reason,
        });
    }
    // Older builds print a bare status word.
    Ok(ProviderAuth {
        provider: provider.to_owned(),
        status: text.lines().next().unwrap_or("unknown").trim().to_owned(),
        auth_type: None,
        reason: None,
    })
}

pub async fn check_auth(config: &AgentConfig, provider: &str) -> Result<ProviderAuth> {
    // `pi auth check` exits non-zero for not_ready/invalid but still reports JSON on stdout.
    let (success, stdout, stderr) = command_output_status(
        config,
        &[
            "auth",
            "check",
            "--provider",
            provider,
            "--json",
            "--no-refresh",
        ],
    )
    .await?;
    parse_auth_report(provider, &stdout).map_err(|error| {
        anyhow::anyhow!(
            "credential check failed (success={success}): {error}; {}",
            stderr.trim()
        )
    })
}

pub async fn discover(kind: &str, config: &AgentConfig) -> Result<Vec<AgentModel>> {
    match kind {
        "codex" => Ok(codex_config_models()),
        "pi" => {
            let configured = pi_config_models();
            let discovered = command_output(config, &["--list-models"])
                .await
                .map(|output| parse_pi_models(&output))
                .unwrap_or_default();
            Ok(merge_models(configured, discovered))
        }
        _ => Ok(Vec::new()),
    }
}

#[cfg(test)]
mod auth_report_tests {
    use super::*;

    #[test]
    fn parses_both_pi_auth_report_shapes() {
        let ready = parse_auth_report(
            "anthropic",
            "{\"status\":\"ready\",\"provider\":\"anthropic\",\"authType\":\"api_key\"}",
        )
        .unwrap();
        assert_eq!(ready.status, "ready");
        assert_eq!(ready.auth_type.as_deref(), Some("api_key"));
        let missing = parse_auth_report(
            "openai",
            "{\"status\":\"not_ready\",\"provider\":\"openai\",\"reason\":\"credentials_not_configured\"}",
        )
        .unwrap();
        assert_eq!(missing.status, "not_ready");
        assert_eq!(
            missing.reason.as_deref(),
            Some("credentials_not_configured")
        );
        let bare = parse_auth_report("openai", "ready\n").unwrap();
        assert_eq!(bare.status, "ready");
        assert!(parse_auth_report("openai", "  ").is_err());
    }

    #[test]
    fn rejects_tokens_that_could_be_flags() {
        assert!(valid_cli_token("anthropic"));
        assert!(valid_cli_token("zai-coding-cn"));
        for rejected in [
            "",
            "-json",
            "--provider",
            "openai; rm -rf /",
            "a b",
            "ünicode",
        ] {
            assert!(!valid_cli_token(rejected), "{rejected:?}");
        }
    }
}

#[cfg(test)]
mod model_list_tests {
    use super::*;

    #[test]
    fn parses_context_window_column() {
        let output = "Provider    Model           Context  Max-out\n\
╭───────────┬───────────────┬────────┬────────╮\n\
  qwen      glm-5.3         128K     16.4K\n\
  agnes     agnes-2.5-flash 1M       128K\n\
  local     my-model        200000   n/a\n\
  broken    no-context      -        n/a\n";
        let models = parse_pi_models(output);
        let window = |id: &str| {
            models
                .iter()
                .find(|m| m.id == id)
                .and_then(|m| m.context_window)
        };
        assert_eq!(window("qwen/glm-5.3"), Some(128_000));
        assert_eq!(window("agnes/agnes-2.5-flash"), Some(1_000_000));
        assert_eq!(window("local/my-model"), Some(200_000));
        assert_eq!(window("broken/no-context"), None);
    }

    #[test]
    fn discovered_window_fills_configured_entry() {
        let configured = vec![AgentModel {
            id: "qwen/glm-5.3".into(),
            name: "glm-5.3".into(),
            provider: Some("qwen".into()),
            source: "/home/u/.pi/agent/models.json".into(),
            context_window: None,
        }];
        let discovered = parse_pi_models("qwen  glm-5.3  128K  16.4K\n");
        let merged = merge_models(configured, discovered);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].context_window, Some(128_000));
        assert_eq!(merged[0].source, "/home/u/.pi/agent/models.json");
    }
}
