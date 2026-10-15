use crate::state::AppState;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::Response,
};
use futures::{SinkExt, StreamExt};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::{info, warn};

#[derive(Debug, Deserialize)]
pub struct TerminalQuery {
    cols: Option<u16>,
    rows: Option<u16>,
    session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum ClientMessage {
    Input { data: String },
    Resize { cols: u16, rows: u16 },
}

pub async fn ws_terminal(
    ws: WebSocketUpgrade,
    Query(query): Query<TerminalQuery>,
    State(state): State<AppState>,
) -> Response {
    let cols = query.cols.unwrap_or(120).clamp(20, 500);
    let rows = query.rows.unwrap_or(32).clamp(5, 200);
    ws.on_upgrade(move |socket| run_terminal(socket, cols, rows, query.session_id, state))
}

async fn run_terminal(
    socket: WebSocket,
    cols: u16,
    rows: u16,
    session_id: Option<String>,
    state: AppState,
) {
    let workspace = if let Some(session_id) = session_id {
        match crate::api::load_session(&state.db, &session_id).await {
            Ok(session) => std::path::PathBuf::from(session.workspace),
            Err(_) => {
                let mut socket = socket;
                let _ = socket
                    .send(Message::Text("Session not found.".into()))
                    .await;
                return;
            }
        }
    } else {
        std::env::var_os("AGENTWEB_WORKSPACE_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("/workspace"))
    };

    if !workspace.is_dir() {
        warn!(path = ?workspace, "terminal workspace does not exist");
        let mut socket = socket;
        let _ = socket
            .send(Message::Text("Terminal workspace does not exist.".into()))
            .await;
        return;
    }

    let pty_system = native_pty_system();
    let pair = match pty_system.openpty(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }) {
        Ok(pair) => pair,
        Err(error) => {
            warn!(%error, "failed to create PTY");
            let mut socket = socket;
            let _ = socket
                .send(Message::Text(
                    format!("Failed to create terminal: {error}").into(),
                ))
                .await;
            return;
        }
    };

    #[cfg(not(target_os = "linux"))]
    {
        let mut socket = socket;
        let _ = socket
            .send(Message::Text(
                "Workspace terminal sandbox is currently supported only on Linux.".into(),
            ))
            .await;
        return;
    }

    let shell = std::env::var("AGENTWEB_TERMINAL_SHELL").unwrap_or_else(|_| "/bin/bash".into());

    #[cfg(not(target_os = "linux"))]
    let mut command = CommandBuilder::new("/bin/false");

    #[cfg(target_os = "linux")]
    let mut command = {
        let workspace = match std::fs::canonicalize(&workspace) {
            Ok(path) if path.is_dir() => path,
            _ => {
                let mut socket = socket;
                let _ = socket
                    .send(Message::Text("Invalid terminal workspace.".into()))
                    .await;
                return;
            }
        };
        if workspace == std::path::Path::new("/workspaces") {
            let mut socket = socket;
            let _ = socket
                .send(Message::Text(
                    "Terminal workspace must be a project directory, not the shared /workspaces root.".into(),
                ))
                .await;
            return;
        }
        let mut command = CommandBuilder::new("bwrap");
        command.arg("--die-with-parent");
        command.arg("--new-session");
        command.arg("--unshare-pid");
        command.arg("--unshare-ipc");
        command.arg("--unshare-uts");
        command.arg("--ro-bind");
        command.arg("/");
        command.arg("/");
        command.arg("--dev");
        command.arg("/dev");
        command.arg("--proc");
        command.arg("/proc");
        command.arg("--tmpfs");
        command.arg("/tmp");
        // Bind the actual host home directly; no /tmp/agentweb-mounts/home
        // source is needed. Do this before masking the host's /home or /data.
        command.arg("--dir");
        command.arg("/agentweb");
        command.arg("--dir");
        command.arg("/agentweb/home");
        let agent_home = std::env::var_os("AGENTWEB_AGENT_HOME")
            .or_else(|| std::env::var_os("HOME"))
            .map(std::path::PathBuf::from)
            .and_then(|path| path.canonicalize().ok());
        let Some(agent_home) = agent_home.filter(|path| path.is_dir()) else {
            let mut socket = socket;
            let _ = socket.send(Message::Text(
                "Agent home directory is unavailable; set AGENTWEB_AGENT_HOME or HOME.".into(),
            )).await;
            return;
        };
        command.arg("--ro-bind");
        command.arg(&agent_home);
        command.arg("/agentweb/home");
        command.arg("--dir");
        command.arg("/tmp/agentweb-mounts");
        command.arg("--dir");
        command.arg("/tmp/agentweb-mounts/runtimes");
        command.arg("--dir");
        command.arg("/tmp/agentweb-mounts/workspace");
        let runtime_root = std::env::var_os("AGENTWEB_RUNTIME_DIR")
            .map(std::path::PathBuf::from)
            .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
            .filter(|path| path.is_dir())
            .and_then(|path| path.canonicalize().ok());
        if let Some(runtime_root) = &runtime_root {
            command.arg("--ro-bind");
            command.arg(runtime_root);
            command.arg("/tmp/agentweb-mounts/runtimes");
        }
        command.arg("--tmpfs");
        command.arg("/data");
        command.arg("--dir");
        command.arg("/data/runtimes");
        if runtime_root.is_some() {
            command.arg("--ro-bind");
            command.arg("/tmp/agentweb-mounts/runtimes");
            command.arg("/data/runtimes");
        }
        command.arg("--tmpfs");
        command.arg("/home");
        command.arg("--tmpfs");
        command.arg("/root");
        command.arg("--tmpfs");
        command.arg("/run");
        command.arg("--tmpfs");
        command.arg("/var");
        command.arg("--tmpfs");
        command.arg("/mnt");
        command.arg("--tmpfs");
        command.arg("/sys");
        if workspace.starts_with("/workspaces") && workspace != std::path::Path::new("/workspaces")
        {
            // Keep the selected project reachable via a temporary alias before
            // hiding /workspaces, so sibling projects are not visible in shell.
            command.arg("--bind");
            command.arg(&workspace);
            command.arg("/tmp/agentweb-mounts/workspace");
            command.arg("--tmpfs");
            command.arg("/workspaces");
            let relative = match workspace.strip_prefix("/workspaces") {
                Ok(path) => path,
                Err(_) => {
                    let mut socket = socket;
                    let _ = socket
                        .send(Message::Text("Invalid terminal workspace path.".into()))
                        .await;
                    return;
                }
            };
            let mut target = std::path::PathBuf::from("/workspaces");
            for component in relative.components() {
                target.push(component.as_os_str());
                command.arg("--dir");
                command.arg(&target);
            }
            command.arg("--bind");
            command.arg("/tmp/agentweb-mounts/workspace");
            command.arg(&workspace);
        } else {
            command.arg("--bind");
            command.arg(&workspace);
            command.arg(&workspace);
        }
        command.arg("--chdir");
        command.arg(&workspace);
        command.arg("--");
        command.arg(&shell);
        command.cwd(&workspace);
        command
    };
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    command.env("HOME", "/agentweb/home");
    command.env("XDG_CONFIG_HOME", "/agentweb/home/.config");
    command.env("XDG_DATA_HOME", "/agentweb/home/.local/share");
    command.env("XDG_CACHE_HOME", "/agentweb/home/.cache");
    command.env("PATH", std::env::var("PATH").unwrap_or_default());
    command.env("SHELL", &shell);

    let mut child = match pair.slave.spawn_command(command) {
        Ok(child) => child,
        Err(error) => {
            warn!(%error, "failed to spawn terminal shell");
            let mut socket = socket;
            let _ = socket
                .send(Message::Text(
                    format!("Failed to start shell: {error}").into(),
                ))
                .await;
            return;
        }
    };
    drop(pair.slave);

    let mut reader = match pair.master.try_clone_reader() {
        Ok(reader) => reader,
        Err(error) => {
            warn!(%error, "failed to clone PTY reader");
            let _ = child.kill();
            return;
        }
    };
    let writer = match pair.master.take_writer() {
        Ok(writer) => writer,
        Err(error) => {
            warn!(%error, "failed to acquire PTY writer");
            let _ = child.kill();
            return;
        }
    };
    let master = pair.master;

    let (output_tx, mut output_rx) = mpsc::channel::<Vec<u8>>(64);
    std::thread::spawn(move || {
        use std::io::Read;
        let mut buffer = [0u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(size) => {
                    if output_tx.blocking_send(buffer[..size].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let (mut sender, mut receiver) = socket.split();
    let mut writer = writer;
    let mut ticker = tokio::time::interval(std::time::Duration::from_millis(250));

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                match child.try_wait() {
                    Ok(Some(_)) => {
                        let _ = sender.send(Message::Text("\r\n[process exited]\r\n".into())).await;
                        break;
                    }
                    Ok(None) => {}
                    Err(_) => break,
                }
            }
            Some(output) = output_rx.recv() => {
                if sender.send(Message::Binary(output.into())).await.is_err() { break; }
            }
            Some(message) = receiver.next() => {
                match message {
                    Ok(Message::Text(text)) => match serde_json::from_str::<ClientMessage>(&text) {
                        Ok(ClientMessage::Input { data }) => {
                            use std::io::Write;
                            if writer.write_all(data.as_bytes()).and_then(|_| writer.flush()).is_err() { break; }
                        }
                        Ok(ClientMessage::Resize { cols, rows }) => {
                            let _ = master.resize(PtySize { rows: rows.clamp(5, 200), cols: cols.clamp(20, 500), pixel_width: 0, pixel_height: 0 });
                        }
                        Err(_) => {
                            use std::io::Write;
                            if writer.write_all(text.as_bytes()).and_then(|_| writer.write_all(b"\n")).and_then(|_| writer.flush()).is_err() { break; }
                        }
                    },
                    Ok(Message::Binary(data)) => {
                        use std::io::Write;
                        if writer.write_all(&data).and_then(|_| writer.flush()).is_err() { break; }
                    }
                    Ok(Message::Close(_)) | Err(_) => break,
                    _ => {}
                }
            }
            else => break,
        }
    }

    let _ = child.kill();
    let _ = child.wait();
    info!(workspace = %workspace.display(), "browser terminal session closed");
}
