use crate::state::AppState;
use axum::{
    extract::{
        ws::{Message, WebSocket},
        Path, State, WebSocketUpgrade,
    },
    response::IntoResponse,
};
use tokio::sync::broadcast;

pub async fn ws_events(
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
    State(s): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| websocket(socket, s, id))
}

async fn websocket(mut socket: WebSocket, s: AppState, session_id: String) {
    // Session events remain transient and are never persisted to SQLite. In multi-worker
    // Docker deployments, the local EventBus receives live events from peer workers through
    // the bounded TCP relay configured by the entrypoint. A relay disconnect or full queue
    // can still lose transient events; reconnecting does not replay missed events.
    let mut events = s.events.subscribe();
    loop {
        let event = tokio::select! {
            event = events.recv() => match event {
                Ok(event) => event,
                // A slow client skipped events; keep streaming the latest state.
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return,
            },
            message = socket.recv() => {
                match message {
                    Some(Ok(Message::Close(_))) | None => return,
                    Some(Ok(_)) => continue,
                    Some(Err(_)) => return,
                }
            }
        };
        if event.session_id() != Some(session_id.as_str()) {
            continue;
        }
        let Ok(payload) = serde_json::to_string(&event) else {
            continue;
        };
        if socket.send(Message::Text(payload.into())).await.is_err() {
            return;
        }
    }
}
