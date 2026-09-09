use crate::{events::AgentEvent, state::AppState};
use axum::{extract::{ws::{Message, WebSocket}, State, WebSocketUpgrade}, response::IntoResponse};

pub async fn ws_events(ws: WebSocketUpgrade, State(s): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| websocket(socket, s))
}

async fn websocket(mut socket: WebSocket, s: AppState) {
    let mut rx = s.events.subscribe();
    while let Ok(event) = rx.recv().await {
        if !matches!(event, AgentEvent::InstallOutput { .. } | AgentEvent::InstallCompleted { .. }) {
            continue;
        }
        let Ok(text) = serde_json::to_string(&event) else { continue };
        if socket.send(Message::Text(text.into())).await.is_err() { break; }
    }
}
