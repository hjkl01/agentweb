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
    // Session events are transient (kept in memory, never written to SQLite), so the
    // socket streams live from the in-memory bus. Each backend worker has its own bus,
    // so a socket must land on the worker that runs the session; the default single
    // worker (AGENTWEB_WORKERS=1) keeps this local.
    // ponytail: per-worker bus, add a shared broker only if AGENTWEB_WORKERS > 1 matters.
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
