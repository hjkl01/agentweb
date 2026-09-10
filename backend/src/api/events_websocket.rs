use axum::{extract::{ws::{Message, WebSocket}, State, WebSocketUpgrade}, response::IntoResponse};
use sqlx::Row;
use crate::state::AppState;

pub async fn ws_events(ws: WebSocketUpgrade, State(s): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| websocket(socket, s))
}

async fn websocket(mut socket: WebSocket, s: AppState) {
    let mut cursor = sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(MAX(id), 0) FROM agent_events"
    )
    .fetch_one(&s.db)
    .await
    .unwrap_or(0);
    let mut ticker = tokio::time::interval(std::time::Duration::from_millis(100));

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let rows = sqlx::query(
                    "SELECT id,payload FROM agent_events WHERE id>? AND event_type IN ('install.output','install.completed') ORDER BY id LIMIT 256"
                )
                .bind(cursor)
                .fetch_all(&s.db)
                .await;
                let Ok(rows) = rows else { continue; };
                for row in rows {
                    let id = row.get::<i64, _>(0);
                    let payload = row.get::<String, _>(1);
                    cursor = id;
                    if socket.send(Message::Text(payload.into())).await.is_err() {
                        return;
                    }
                }
            }
            message = socket.recv() => {
                match message {
                    Some(Ok(Message::Close(_))) | None => return,
                    Some(Ok(_)) => {},
                    Some(Err(_)) => return,
                }
            }
        }
    }
}
