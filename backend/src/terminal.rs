use axum::{extract::{ws::{Message, WebSocket, WebSocketUpgrade}, Query}, response::Response};
use futures::{SinkExt, StreamExt};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::{info, warn};

#[derive(Debug, Deserialize)]
pub struct TerminalQuery {
    cols: Option<u16>,
    rows: Option<u16>,
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
) -> Response {
    let cols = query.cols.unwrap_or(120).clamp(20, 500);
    let rows = query.rows.unwrap_or(32).clamp(5, 200);
    ws.on_upgrade(move |socket| run_terminal(socket, cols, rows))
}

async fn run_terminal(socket: WebSocket, cols: u16, rows: u16) {
    let workspace = std::env::var_os("AGENTWEB_WORKSPACE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/workspace"));

    if !workspace.is_dir() {
        warn!(path = ?workspace, "terminal workspace does not exist");
        let mut socket = socket;
        let _ = socket.send(Message::Text("Terminal workspace does not exist.".into())).await;
        return;
    }

    let pty_system = native_pty_system();
    let pair = match pty_system.openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }) {
        Ok(pair) => pair,
        Err(error) => {
            warn!(%error, "failed to create PTY");
            let mut socket = socket;
            let _ = socket.send(Message::Text(format!("Failed to create terminal: {error}").into())).await;
            return;
        }
    };

    let shell = std::env::var("AGENTWEB_TERMINAL_SHELL").unwrap_or_else(|_| "/bin/bash".into());
    let mut command = CommandBuilder::new(&shell);
    command.cwd(&workspace);
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    command.env("HOME", std::env::var("HOME").unwrap_or_else(|_| "/data/home".into()));
    command.env("PATH", std::env::var("PATH").unwrap_or_default());
    command.env("SHELL", &shell);

    let mut child = match pair.slave.spawn_command(command) {
        Ok(child) => child,
        Err(error) => {
            warn!(%error, "failed to spawn terminal shell");
            let mut socket = socket;
            let _ = socket.send(Message::Text(format!("Failed to start shell: {error}").into())).await;
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
                    if output_tx.blocking_send(buffer[..size].to_vec()).is_err() { break; }
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
    info!("browser terminal session closed");
}
