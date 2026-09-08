use crate::{events::AgentEvent, state::AppState};
use axum::{extract::{ws::{Message, WebSocket}, Path, State, WebSocketUpgrade}, response::IntoResponse};

fn event_session_id(event: &AgentEvent) -> Option<&str> {
    match event {
        AgentEvent::SessionStarted { session_id } | AgentEvent::MessageStarted { session_id } | AgentEvent::MessageDelta { session_id, .. } | AgentEvent::MessageCompleted { session_id } | AgentEvent::ThinkingStarted { session_id } | AgentEvent::ThinkingDelta { session_id, .. } | AgentEvent::ThinkingCompleted { session_id } | AgentEvent::ToolStarted { session_id, .. } | AgentEvent::ToolOutput { session_id, .. } | AgentEvent::ToolCompleted { session_id, .. } | AgentEvent::FileCreated { session_id, .. } | AgentEvent::FileModified { session_id, .. } | AgentEvent::FileDeleted { session_id, .. } | AgentEvent::CommandStarted { session_id, .. } | AgentEvent::CommandOutput { session_id, .. } | AgentEvent::CommandCompleted { session_id } | AgentEvent::Error { session_id, .. } | AgentEvent::SessionCompleted { session_id } => Some(session_id),
        AgentEvent::InstallOutput { .. } | AgentEvent::InstallCompleted { .. } => None,
    }
}

pub async fn ws_events(Path(id): Path<String>, ws: WebSocketUpgrade, State(s): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| websocket(socket, s, id))
}

async fn websocket(mut socket: WebSocket, s: AppState, session_id: String) {
    let mut rx=s.events.subscribe();
    while let Ok(event)=rx.recv().await {
        if event_session_id(&event)!=Some(session_id.as_str()) { continue; }
        let Ok(text)=serde_json::to_string(&event) else { continue };
        if socket.send(Message::Text(text.into())).await.is_err() { break; }
    }
}
