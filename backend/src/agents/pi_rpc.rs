//! Minimal one-shot client for Pi's RPC mode (`pi --mode rpc`), used for the operations the
//! command line does not expose: forking a session at a chosen entry and listing the skills,
//! prompt templates, and extension commands Pi would load.
//!
//! Protocol records are JSONL: one JSON object per line, commands on stdin, `response` records
//! and session events on stdout, diagnostics on stderr (see `docs/rpc.md` upstream). Every call
//! sends a few commands, reads until the answers arrive, then closes stdin — which is Pi's
//! orderly shutdown request. The child is killed on drop either way.

use super::adapter::AgentConfig;
use super::commands::{apply_agent_environment, command_parts};
use super::models::valid_cli_token;
use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::process::{ChildStdin, ChildStdout};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    process::{Child, Command},
    task::JoinHandle,
};

/// Forks are local file operations, but Pi still starts a full runtime first.
const RPC_TIMEOUT: Duration = Duration::from_secs(60);
/// Enough of Pi's stderr to explain a startup failure, without buffering a whole log.
const MAX_DIAGNOSTIC_BYTES: usize = 8 * 1024;

#[derive(Debug)]
pub struct ForkResult {
    /// The text of the message being forked from, to put back in the editor.
    pub text: Option<String>,
    /// An extension cancelled the fork, so nothing was created.
    pub cancelled: bool,
    pub session_id: Option<String>,
    /// Missing on disk for a fork before the first message: Pi only writes the copy once it
    /// has something to store, so that session is effectively brand new.
    pub session_file: Option<PathBuf>,
}

/// A skill, prompt template, or extension command Pi would expose in a session.
#[derive(Debug, Serialize, PartialEq)]
pub struct CommandInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `skill`, `prompt`, or `extension`.
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// `top-level` for a directly configured resource, `package` for one a Pi package ships.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_dir: Option<String>,
}

/// Fork `config`'s native session at `entry_id`, leaving the source session untouched.
pub async fn fork_at_entry(config: &AgentConfig, entry_id: &str) -> Result<ForkResult> {
    let entry_id = entry_id.trim();
    if entry_id.is_empty() {
        bail!("entry id is required");
    }
    // The entry id travels inside the JSON record, so only the session id reaches argv.
    let session = config
        .native_session_id
        .as_deref()
        .ok_or_else(|| anyhow!("session has no native session id to fork"))?;
    if !valid_cli_token(session) {
        bail!("invalid native session id");
    }
    let mut rpc = RpcSession::start(config, &["--session", session]).await?;
    let exchange = async {
        rpc.send(json!({"id": FORK_ID, "type": "fork", "entryId": entry_id}))
            .await?;
        let fork = rpc.read(FORK_ID).await?;
        // `get_state` is only asked after the fork answer: Pi reports the state that was
        // current when a command was read, so asking both at once returns the source session.
        let state = if fork["success"] == json!(true) && fork["data"]["cancelled"] != json!(true) {
            rpc.send(json!({"id": STATE_ID, "type": "get_state"}))
                .await?;
            Some(rpc.read(STATE_ID).await?)
        } else {
            None
        };
        Ok::<_, anyhow::Error>((fork, state))
    };
    let result = match tokio::time::timeout(RPC_TIMEOUT, exchange).await {
        Ok(Ok(answers)) => Ok(answers),
        Ok(Err(error)) => Err(error.to_string()),
        Err(_) => Err(format!("fork timed out after {RPC_TIMEOUT:?}.")),
    };
    let diagnostics = rpc.stop().await;
    match result {
        Ok((fork, state)) => fork_result(&fork, state.as_ref()),
        Err(message) => Err(anyhow!("{message}{diagnostics}")),
    }
}

/// Every skill, prompt template, and extension command Pi loads for `config`'s cwd.
///
/// The listing is Pi's own resource discovery (settings, packages, project ancestors, trust),
/// so an untrusted project simply contributes nothing — the same files a run would load.
pub async fn list_commands(config: &AgentConfig) -> Result<Vec<CommandInfo>> {
    // No session file and no MCP connections: nothing here needs an agent runtime.
    let mut rpc = RpcSession::start(config, &["--no-session", "--no-mcp"]).await?;
    let exchange = async {
        rpc.send(json!({"id": COMMANDS_ID, "type": "get_commands"}))
            .await?;
        commands(&rpc.read(COMMANDS_ID).await?)
    };
    let result = match tokio::time::timeout(RPC_TIMEOUT, exchange).await {
        Ok(Ok(commands)) => Ok(commands),
        Ok(Err(error)) => Err(error.to_string()),
        Err(_) => Err(format!(
            "listing Pi commands timed out after {RPC_TIMEOUT:?}."
        )),
    };
    let diagnostics = rpc.stop().await;
    result.map_err(|message| anyhow!("{message}{diagnostics}"))
}

const FORK_ID: &str = "fork";
const STATE_ID: &str = "state";
const COMMANDS_ID: &str = "commands";

/// One Pi process in RPC mode, with its stdout stream and its stderr diagnostics task.
struct RpcSession {
    child: Child,
    stdin: ChildStdin,
    lines: tokio::io::Lines<BufReader<ChildStdout>>,
    diagnostics: JoinHandle<String>,
}

impl RpcSession {
    /// Starts Pi with `args` appended after `--mode rpc`.
    async fn start(config: &AgentConfig, args: &[&str]) -> Result<Self> {
        let (program, base) = command_parts(&config.command)?;
        let mut command = Command::new(&program);
        command
            .args(base)
            .args(["--mode", "rpc"])
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        apply_agent_environment(config, &mut command, None);
        if let Some(dir) = &config.working_directory {
            command.current_dir(dir);
        }
        let mut child = command
            .spawn()
            .with_context(|| format!("cannot start {program} in RPC mode"))?;
        let diagnostics = tokio::spawn(read_diagnostics(
            child.stderr.take().expect("stderr is piped"),
        ));
        let stdin = child.stdin.take().expect("stdin is piped");
        let stdout = child.stdout.take().expect("stdout is piped");
        Ok(Self {
            child,
            stdin,
            lines: BufReader::new(stdout).lines(),
            diagnostics,
        })
    }

    async fn send(&mut self, record: Value) -> Result<()> {
        send(&mut self.stdin, record).await
    }

    async fn read(&mut self, id: &str) -> Result<Value> {
        read_response(&mut self.lines, id).await
    }

    /// Stops the runtime and returns its diagnostics, formatted for an error message.
    async fn stop(mut self) -> String {
        // Closing stdin is Pi's orderly shutdown request; killing is the fallback.
        drop(self.stdin);
        let _ = self.child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(5), self.child.wait()).await;
        let diagnostics = self.diagnostics.await.unwrap_or_default();
        let diagnostics = diagnostics.trim();
        if diagnostics.is_empty() {
            String::new()
        } else {
            format!(" Pi 输出：{diagnostics}")
        }
    }
}

fn commands(response: &Value) -> Result<Vec<CommandInfo>> {
    if response["success"] != json!(true) {
        bail!(
            "{}",
            response["error"]
                .as_str()
                .unwrap_or("Pi rejected the command listing")
        );
    }
    let items = response["data"]["commands"].as_array();
    Ok(items
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let name = item["name"].as_str()?.trim();
            if name.is_empty() {
                return None;
            }
            let info = &item["sourceInfo"];
            let text = |value: &Value| {
                value
                    .as_str()
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(str::to_owned)
            };
            Some(CommandInfo {
                name: name.to_owned(),
                description: text(&item["description"]),
                source: text(&item["source"]).unwrap_or_else(|| "other".into()),
                scope: text(&info["scope"]),
                path: text(&info["path"]),
                origin: text(&info["origin"]),
                base_dir: text(&info["baseDir"]),
            })
        })
        .collect())
}

async fn read_diagnostics(stderr: tokio::process::ChildStderr) -> String {
    let mut buffer = String::new();
    let mut lines = BufReader::new(stderr).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if buffer.len() >= MAX_DIAGNOSTIC_BYTES {
            break;
        }
        buffer.push_str(&line);
        buffer.push('\n');
    }
    buffer
}

/// Writes one command record.
async fn send<W: AsyncWrite + Unpin>(stdin: &mut W, record: Value) -> Result<()> {
    let mut line = record.to_string();
    line.push('\n');
    stdin.write_all(line.as_bytes()).await?;
    stdin.flush().await?;
    Ok(())
}

/// Reads until the answer to `id` arrives, ignoring the session events that share the stream
/// (they have no command id). Pi answers a malformed record with a `parse` response instead.
async fn read_response<R: AsyncBufRead + Unpin>(
    lines: &mut tokio::io::Lines<R>,
    id: &str,
) -> Result<Value> {
    while let Some(line) = lines.next_line().await? {
        let Ok(record) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if record["type"] != "response" {
            continue;
        }
        if record["id"].as_str() != Some(id) {
            bail!(
                "unexpected RPC response: {}",
                record["error"].as_str().unwrap_or("unknown error")
            );
        }
        return Ok(record);
    }
    bail!("Pi closed its RPC stream before answering")
}

fn fork_result(fork: &Value, state: Option<&Value>) -> Result<ForkResult> {
    if fork["success"] != json!(true) {
        bail!(
            "{}",
            fork["error"]
                .as_str()
                .unwrap_or("Pi rejected the fork command")
        );
    }
    let data = &fork["data"];
    if data["cancelled"] == json!(true) {
        return Ok(ForkResult {
            text: None,
            cancelled: true,
            session_id: None,
            session_file: None,
        });
    }
    let state =
        &state.ok_or_else(|| anyhow!("Pi did not report the forked session state"))?["data"];
    Ok(ForkResult {
        text: data["text"].as_str().map(str::to_owned),
        cancelled: false,
        session_id: state["sessionId"].as_str().map(str::to_owned),
        session_file: state["sessionFile"].as_str().map(PathBuf::from),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn read(stream: &str, id: &str) -> Result<Value> {
        read_response(&mut BufReader::new(stream.as_bytes()).lines(), id).await
    }

    #[tokio::test]
    async fn reads_a_response_across_interleaved_events() {
        // Records taken from a real `pi --mode rpc` exchange: events share the stream.
        let stream = concat!(
            "{\"type\":\"session_start\"}\n",
            "{\"id\":\"fork\",\"type\":\"response\",\"command\":\"fork\",\"success\":true,",
            "\"data\":{\"text\":\"second question\",\"cancelled\":false}}\n",
        );
        let fork = read(stream, FORK_ID).await.unwrap();
        assert_eq!(fork["data"]["text"], json!("second question"));

        let state = concat!(
            "{\"type\":\"message_update\"}\n",
            "{\"id\":\"state\",\"type\":\"response\",\"command\":\"get_state\",\"success\":true,",
            "\"data\":{\"sessionId\":\"01a15dd7\",\"sessionFile\":\"/tmp/fork.jsonl\"}}\n",
        );
        let state = read(state, STATE_ID).await.unwrap();
        let result = fork_result(&fork, Some(&state)).unwrap();
        assert!(!result.cancelled);
        assert_eq!(result.text.as_deref(), Some("second question"));
        assert_eq!(result.session_id.as_deref(), Some("01a15dd7"));
        assert_eq!(
            result.session_file.as_deref(),
            Some(std::path::Path::new("/tmp/fork.jsonl"))
        );
    }

    #[tokio::test]
    async fn reports_a_rejection_a_parse_error_and_a_truncated_stream() {
        let rejected = concat!(
            "{\"id\":\"fork\",\"type\":\"response\",\"command\":\"fork\",\"success\":false,",
            "\"error\":\"Invalid entry ID for forking\"}\n",
        );
        let fork = read(rejected, FORK_ID).await.unwrap();
        let error = fork_result(&fork, None).unwrap_err().to_string();
        assert!(error.contains("Invalid entry ID"), "{error}");

        // A malformed record is answered by Pi as a `parse` response with no command id.
        let parse_error = "{\"type\":\"response\",\"command\":\"parse\",\"success\":false,\"error\":\"Failed to parse command\"}\n";
        let error = read(parse_error, FORK_ID).await.unwrap_err().to_string();
        assert!(error.contains("Failed to parse command"), "{error}");

        let error = read("", FORK_ID).await.unwrap_err().to_string();
        assert!(error.contains("closed its RPC stream"), "{error}");
    }

    #[test]
    fn parses_commands_and_skips_malformed_entries() {
        // Shape taken from a real `get_commands` answer (docs/rpc-commands.md upstream).
        let response = json!({
            "type": "response",
            "success": true,
            "data": {"commands": [
                {"name": "skill:demo", "description": " A demo ", "source": "skill",
                 "sourceInfo": {"path": "/p/.pi/skills/demo/SKILL.md", "scope": "project",
                                 "origin": "top-level", "baseDir": "/p/.pi"}},
                {"name": "", "source": "skill"},
                {"description": "no name"},
                {"name": "ponytail", "source": "extension",
                 "sourceInfo": {"origin": "package", "baseDir": "/pkg"}},
            ]},
        });
        let parsed = commands(&response).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].name, "skill:demo");
        assert_eq!(parsed[0].description.as_deref(), Some("A demo"));
        assert_eq!(parsed[0].scope.as_deref(), Some("project"));
        assert_eq!(parsed[0].base_dir.as_deref(), Some("/p/.pi"));
        // A missing scope/path stays absent instead of failing the whole listing.
        assert_eq!(parsed[1].name, "ponytail");
        assert!(parsed[1].description.is_none());
        assert_eq!(parsed[1].origin.as_deref(), Some("package"));

        let failed = json!({"success": false, "error": "boom"});
        assert_eq!(commands(&failed).unwrap_err().to_string(), "boom");
    }

    #[test]
    fn a_cancelled_fork_creates_nothing() {
        let fork = json!({"type":"response","success":true,"data":{"cancelled":true}});
        let result = fork_result(&fork, None).unwrap();
        assert!(result.cancelled);
        assert!(result.session_id.is_none());
    }
}
