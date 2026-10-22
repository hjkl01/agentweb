//! Thin wrapper over `pi mcp …`. AgentWeb never speaks MCP itself: it only reads the
//! server list Pi already resolved and delegates config edits back to Pi, so `mcp.json`
//! stays owned by Pi.

use super::adapter::AgentConfig;
use super::models::{run_agent_command, valid_cli_token};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

/// `pi mcp list` connects to every enabled server before answering.
const MCP_TIMEOUT: Duration = Duration::from_secs(60);
const EXPOSURES: &[&str] = &["codemode", "deferred", "direct", "hidden"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServer {
    pub name: String,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub exposure: Option<String>,
    #[serde(default)]
    pub transport: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpView {
    #[serde(default)]
    pub servers: Vec<McpServer>,
    #[serde(default)]
    pub errors: Vec<Value>,
}

pub async fn list(config: &AgentConfig) -> Result<McpView> {
    let (success, stdout, stderr) =
        run_agent_command(config, &["mcp", "list", "--json"], Some(MCP_TIMEOUT)).await?;
    let text = stdout.trim();
    if text.is_empty() {
        return Err(anyhow::anyhow!(
            "pi mcp list produced no output (success={success}): {}",
            stderr.trim()
        ));
    }
    serde_json::from_str::<McpView>(text).map_err(|error| {
        anyhow::anyhow!(
            "unexpected pi mcp list output: {error}: {}",
            text.chars().take(200).collect::<String>()
        )
    })
}

/// An option value must never look like a flag: `pi mcp` rejects `--flag=value`
/// syntax, so every value is passed as its own argv entry.
fn plain(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.starts_with('-') || value.contains('\0') {
        return None;
    }
    Some(value.to_owned())
}

/// `KEY=VALUE` for `--env`.
fn env_entry(value: &str) -> Option<String> {
    let (key, _) = value.split_once('=')?;
    let key = key.trim();
    if key.is_empty()
        || key.starts_with('-')
        || !key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
    {
        return None;
    }
    plain(value)
}

/// `KEY=VALUE` for `--header`. Pi rejects an `Authorization` header here because a raw
/// header would persist the secret in `mcp.json`; `--bearer-token-env-var` is the
/// supported alternative.
fn header_entry(value: &str) -> Option<String> {
    if value.contains('\n') {
        return None;
    }
    let key = value.split_once('=')?.0.trim().to_ascii_lowercase();
    if key == "authorization" {
        return None;
    }
    env_entry(value)
}

/// Bare environment variable name, e.g. for `--bearer-token-env-var`.
fn env_var_name(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.starts_with('-') {
        return None;
    }
    value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
        .then(|| value.to_owned())
}

fn https_url(value: &str) -> Option<String> {
    let value = value.trim();
    (value.starts_with("http://") || value.starts_with("https://"))
        .then(|| plain(value))
        .flatten()
}

#[derive(Debug, Deserialize)]
pub struct McpAddRequest {
    pub name: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(default)]
    pub headers: Vec<String>,
    #[serde(default)]
    pub bearer_token_env_var: Option<String>,
    #[serde(default)]
    pub exposure: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub oauth_callback_port: Option<u16>,
}

impl McpAddRequest {
    /// Builds the `pi mcp add` argv. Returns a user-facing message on invalid input.
    pub fn arguments(&self) -> std::result::Result<Vec<String>, String> {
        if !valid_cli_token(&self.name) {
            return Err("服务器名称只能包含字母、数字、-、_、.、/，且不能以 - 开头".into());
        }
        let url = self.url.as_deref().map(str::trim).filter(|v| !v.is_empty());
        let command = self
            .command
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty());
        if url.is_some() == command.is_some() {
            return Err("必须且只能提供一种传输方式：URL 或命令".into());
        }

        let mut args = vec!["mcp".to_owned(), "add".to_owned(), self.name.clone()];
        if let Some(exposure) = &self.exposure {
            if !EXPOSURES.contains(&exposure.as_str()) {
                return Err(format!("exposure 必须是 {} 之一", EXPOSURES.join(" / ")));
            }
            args.push("--exposure".into());
            args.push(exposure.clone());
        }
        if let Some(description) = &self.description {
            args.push("--description".into());
            args.push(plain(description).ok_or("描述不能为空或以 - 开头")?);
        }

        if let Some(url) = url {
            args.push("--url".into());
            args.push(https_url(url).ok_or("URL 必须以 http:// 或 https:// 开头")?);
            for header in &self.headers {
                args.push("--header".into());
                args.push(header_entry(header).ok_or_else(|| {
                    format!("请求头需要 KEY=VALUE 形式且不能是 Authorization：{header}")
                })?);
            }
            if let Some(name) = &self.bearer_token_env_var {
                args.push("--bearer-token-env-var".into());
                args.push(env_var_name(name).ok_or("bearer token 环境变量名不合法")?);
            }
            if let Some(port) = self.oauth_callback_port.filter(|port| *port > 0) {
                args.push("--oauth-callback-port".into());
                args.push(port.to_string());
            }
            return Ok(args);
        }

        for entry in &self.env {
            args.push("--env".into());
            args.push(
                env_entry(entry).ok_or_else(|| format!("环境变量需要 KEY=VALUE 形式：{entry}"))?,
            );
        }
        let command = command.expect("one transport is present");
        if command.contains('\0') {
            return Err("命令不合法".into());
        }
        // Everything after `--` is handed to the server verbatim.
        args.push("--".into());
        args.push(command.to_owned());
        if let Some(bad) = self.args.iter().find(|arg| arg.contains('\0')) {
            return Err(format!("参数不合法：{bad}"));
        }
        args.extend(self.args.iter().cloned());
        Ok(args)
    }
}

/// Errors carry Pi's own message so the caller can surface it verbatim.
async fn run(config: &AgentConfig, extra: Vec<String>) -> Result<()> {
    let refs: Vec<&str> = extra.iter().map(String::as_str).collect();
    let (success, stdout, stderr) = run_agent_command(config, &refs, Some(MCP_TIMEOUT)).await?;
    if !success {
        let message = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        return Err(anyhow::anyhow!("{message}"));
    }
    Ok(())
}

/// Every mutation re-reads the resolved list so the caller sees the new state at once.
pub async fn add(config: &AgentConfig, args: Vec<String>) -> Result<McpView> {
    run(config, args).await?;
    list(config).await
}

pub async fn remove(config: &AgentConfig, name: &str) -> Result<McpView> {
    if !valid_cli_token(name) {
        anyhow::bail!("服务器名称不合法");
    }
    run(config, vec!["mcp".into(), "remove".into(), name.to_owned()]).await?;
    list(config).await
}

pub async fn logout(config: &AgentConfig, name: &str) -> Result<McpView> {
    if !valid_cli_token(name) {
        anyhow::bail!("服务器名称不合法");
    }
    run(config, vec!["mcp".into(), "logout".into(), name.to_owned()]).await?;
    list(config).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stdio(name: &str, command: &str, args: &[&str]) -> McpAddRequest {
        McpAddRequest {
            name: name.into(),
            url: None,
            command: Some(command.into()),
            args: args.iter().map(|arg| (*arg).into()).collect(),
            env: vec![],
            headers: vec![],
            bearer_token_env_var: None,
            exposure: None,
            description: None,
            oauth_callback_port: None,
        }
    }

    #[test]
    fn builds_stdio_arguments_with_a_double_dash_separator() {
        let args = stdio("demo", "node", &["server.js", "--flag", "x"])
            .arguments()
            .unwrap();
        assert_eq!(args[..3], ["mcp", "add", "demo"]);
        let at = args.iter().position(|arg| arg == "--").unwrap();
        assert_eq!(&args[at + 1..], ["node", "server.js", "--flag", "x"]);
    }

    #[test]
    fn builds_http_arguments_and_keeps_secrets_out_of_mcp_json() {
        let mut request = stdio("remote", "", &[]);
        request.command = None;
        request.url = Some("https://example.com/mcp".into());
        request.headers = vec!["X-Tenant=acme".into()];
        request.bearer_token_env_var = Some("REMOTE_TOKEN".into());
        let args = request.arguments().unwrap();
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--url", "https://example.com/mcp"]));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--header", "X-Tenant=acme"]));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--bearer-token-env-var", "REMOTE_TOKEN"]));
        // An explicit Authorization header would persist the token in mcp.json.
        request.headers = vec!["Authorization=Bearer abc".into()];
        assert!(request.arguments().is_err());
        request.headers = vec!["X-Tenant: acme".into()];
        assert!(request.arguments().is_err());
    }

    #[test]
    fn parses_pi_mcp_list_output() {
        let sample = r#"{
          "servers": [
            {"name":"fake","scope":"global","source":"/tmp/mcp.json","enabled":true,
             "exposure":"codemode","transport":"node /tmp/fake.js","state":"connected","tools":["echo"]},
            {"name":"broken","scope":"global","source":"/tmp/mcp.json","enabled":true,
             "exposure":"direct","transport":"http://127.0.0.1:1/mcp","state":"failed","tools":[],"error":"fetch failed"}
          ],
          "errors": []
        }"#;
        let view: McpView = serde_json::from_str(sample).unwrap();
        assert_eq!(view.servers.len(), 2);
        assert_eq!(view.servers[0].state.as_deref(), Some("connected"));
        assert_eq!(view.servers[0].tools, ["echo"]);
        assert_eq!(view.servers[1].error.as_deref(), Some("fetch failed"));
        // A minimal entry must not fail the whole list.
        let sparse: McpView = serde_json::from_str(r#"{"servers":[{"name":"x"}]}"#).unwrap();
        assert!(!sparse.servers[0].enabled);
        assert!(sparse.servers[0].tools.is_empty());
    }

    #[test]
    fn rejects_inputs_that_could_be_read_as_flags() {
        assert!(stdio("--local", "node", &[]).arguments().is_err());
        assert!(stdio("demo", "node", &[]).arguments().is_ok());
        // Two transports at once.
        let mut request = stdio("demo", "node", &[]);
        request.url = Some("https://example.com/mcp".into());
        assert!(request.arguments().is_err());
        // Neither transport.
        request.command = None;
        request.url = None;
        assert!(request.arguments().is_err());
        // A value that pi would parse as a flag.
        let mut request = stdio("demo", "node", &[]);
        request.command = None;
        request.url = Some("-url".into());
        assert!(request.arguments().is_err());
        request.url = Some("https://example.com/mcp".into());
        request.description = Some("-oops".into());
        assert!(request.arguments().is_err());
        let mut request = stdio("demo", "node", &[]);
        request.env = vec!["-X=1".into()];
        assert!(request.arguments().is_err());
        request.env = vec!["NO_EQUALS".into()];
        assert!(request.arguments().is_err());
        let mut request = stdio("demo", "node", &[]);
        request.exposure = Some("everywhere".into());
        assert!(request.arguments().is_err());
    }
}
