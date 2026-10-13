use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use futures::{SinkExt, StreamExt};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{broadcast, mpsc},
    time::{sleep, Duration},
};
use tokio_util::codec::{Framed, LinesCodec};

const MAX_RELAY_EVENT_BYTES: usize = 1024 * 1024;
const RELAY_QUEUE_CAPACITY: usize = 256;

#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<AgentEvent>,
    persist_sender: mpsc::Sender<PersistedEvent>,
    worker_id: Option<String>,
    relay_sender: Option<mpsc::Sender<AgentEvent>>,
}

struct PersistedEvent {
    session_id: Option<String>,
    worker_id: Option<String>,
    event_type: String,
    payload: String,
    created_at: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum AgentEvent {
    #[serde(rename = "session.started")]
    SessionStarted { session_id: String },
    #[serde(rename = "message.started")]
    MessageStarted { session_id: String },
    #[serde(rename = "message.delta")]
    MessageDelta { session_id: String, text: String },
    #[serde(rename = "message.completed")]
    MessageCompleted { session_id: String },
    #[serde(rename = "thinking.started")]
    ThinkingStarted { session_id: String },
    #[serde(rename = "thinking.delta")]
    ThinkingDelta { session_id: String, text: String },
    #[serde(rename = "thinking.completed")]
    ThinkingCompleted { session_id: String },
    #[serde(rename = "tool.started")]
    ToolStarted { session_id: String, tool: String },
    #[serde(rename = "tool.output")]
    ToolOutput {
        session_id: String,
        tool: String,
        output: String,
    },
    #[serde(rename = "tool.completed")]
    ToolCompleted { session_id: String, tool: String },
    #[serde(rename = "file.created")]
    FileCreated { session_id: String, path: String },
    #[serde(rename = "file.modified")]
    FileModified { session_id: String, path: String },
    #[serde(rename = "file.deleted")]
    FileDeleted { session_id: String, path: String },
    #[serde(rename = "command.started")]
    CommandStarted { session_id: String, command: String },
    #[serde(rename = "command.output")]
    CommandOutput { session_id: String, output: String },
    #[serde(rename = "command.completed")]
    CommandCompleted { session_id: String },
    #[serde(rename = "agent.error")]
    Error { session_id: String, message: String },
    #[serde(rename = "session.completed")]
    SessionCompleted { session_id: String },
    #[serde(rename = "install.output")]
    InstallOutput { agent_id: String, text: String },
    #[serde(rename = "install.completed")]
    InstallCompleted { agent_id: String, success: bool },
}

impl AgentEvent {
    pub fn session_id(&self) -> Option<&str> {
        match self {
            AgentEvent::SessionStarted { session_id }
            | AgentEvent::MessageStarted { session_id }
            | AgentEvent::MessageDelta { session_id, .. }
            | AgentEvent::MessageCompleted { session_id }
            | AgentEvent::ThinkingStarted { session_id }
            | AgentEvent::ThinkingDelta { session_id, .. }
            | AgentEvent::ThinkingCompleted { session_id }
            | AgentEvent::ToolStarted { session_id, .. }
            | AgentEvent::ToolOutput { session_id, .. }
            | AgentEvent::ToolCompleted { session_id, .. }
            | AgentEvent::FileCreated { session_id, .. }
            | AgentEvent::FileModified { session_id, .. }
            | AgentEvent::FileDeleted { session_id, .. }
            | AgentEvent::CommandStarted { session_id, .. }
            | AgentEvent::CommandOutput { session_id, .. }
            | AgentEvent::CommandCompleted { session_id }
            | AgentEvent::Error { session_id, .. }
            | AgentEvent::SessionCompleted { session_id } => Some(session_id),
            AgentEvent::InstallOutput { .. } | AgentEvent::InstallCompleted { .. } => None,
        }
    }
}

// SQLite primary result codes 5 and 6 are SQLITE_BUSY and SQLITE_LOCKED.
// Extended result codes preserve the primary code in the low 8 bits.
fn is_retryable_sqlite_code(code: Option<&str>) -> bool {
    code.and_then(|value| value.parse::<i32>().ok())
        .map(|value| matches!(value & 0xff, 5 | 6))
        .unwrap_or(false)
}

fn is_retryable_sqlite_error(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Database(database_error) => {
            is_retryable_sqlite_code(database_error.code().as_deref())
        }
        _ => false,
    }
}

// Only SQLite lock contention is retried. A failed commit is never retried
// because the database may have committed despite the error.
async fn persist_batch(
    db: &SqlitePool,
    batch: &[PersistedEvent],
) -> Result<(), (sqlx::Error, bool)> {
    let mut transaction = db.begin().await.map_err(|error| {
        let retryable = is_retryable_sqlite_error(&error);
        (error, retryable)
    })?;
    for event in batch {
        if let Err(error) = sqlx::query(
            "INSERT INTO agent_events(session_id,worker_id,event_type,payload,created_at)
             SELECT ?,?,?,?,?
             WHERE ? IS NULL OR EXISTS (SELECT 1 FROM sessions WHERE id=?)",
        )
        .bind(&event.session_id)
        .bind(&event.worker_id)
        .bind(&event.event_type)
        .bind(&event.payload)
        .bind(&event.created_at)
        .bind(event.session_id.as_deref())
        .bind(event.session_id.as_deref())
        .execute(&mut *transaction)
        .await
        {
            if let Err(rollback_error) = transaction.rollback().await {
                tracing::warn!(%rollback_error, "failed to roll back agent event batch");
            }
            let retryable = is_retryable_sqlite_error(&error);
            return Err((error, retryable));
        }
    }
    transaction.commit().await.map_err(|error| (error, false))
}

impl EventBus {
    pub fn new(db: SqlitePool) -> Self {
        let (persist_sender, mut persist_receiver) = mpsc::channel::<PersistedEvent>(2048);
        tokio::spawn(async move {
            // Batch nearby events into one transaction while preserving publication order.
            // The short deadline keeps low-volume events responsive; the size limit bounds
            // transaction duration during high-volume token streaming.
            while let Some(first) = persist_receiver.recv().await {
                let mut batch = Vec::with_capacity(100);
                batch.push(first);
                let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(5);
                while batch.len() < 100 {
                    match tokio::time::timeout_at(deadline, persist_receiver.recv()).await {
                        Ok(Some(event)) => batch.push(event),
                        Ok(None) | Err(_) => break,
                    }
                }

                let mut attempt = 0_u32;
                loop {
                    match persist_batch(&db, &batch).await {
                        Ok(()) => break,
                        Err((error, retryable)) if retryable && attempt < 5 => {
                            let delay_ms = 25_u64 * (1_u64 << attempt);
                            attempt += 1;
                            tracing::warn!(
                                %error,
                                attempt,
                                delay_ms,
                                "agent event batch persistence failed; retrying"
                            );
                            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        }
                        Err((error, retryable)) => {
                            if retryable {
                                tracing::error!(
                                    %error,
                                    attempts = attempt + 1,
                                    "dropping agent event batch after persistence retries were exhausted"
                                );
                            } else {
                                tracing::error!(
                                    %error,
                                    "agent event batch commit failed; not retrying because commit outcome may be ambiguous"
                                );
                            }
                            break;
                        }
                    }
                }
            }
        });
        Self {
            sender: broadcast::channel(512).0,
            persist_sender,
            worker_id: None,
            relay_sender: None,
        }
    }

    pub fn for_worker(&self, worker_id: String) -> Self {
        Self {
            sender: self.sender.clone(),
            persist_sender: self.persist_sender.clone(),
            worker_id: Some(worker_id),
            relay_sender: self.relay_sender.clone(),
        }
    }

    /// Enable best-effort live event relay between backend workers. Conversation
    /// events remain transient and are never written to SQLite by this relay.
    pub fn with_relay_from_env(mut self) -> Self {
        let listen_addr = std::env::var("AGENTWEB_EVENT_LISTEN_ADDR").ok();
        let peers = std::env::var("AGENTWEB_EVENT_PEERS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|peer| !peer.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let Some(listen_addr) = listen_addr.filter(|addr| !addr.trim().is_empty()) else {
            return self;
        };
        if peers.is_empty() {
            return self;
        }

        let (relay_sender, mut relay_receiver) = mpsc::channel::<AgentEvent>(1024);
        let local_sender = self.sender.clone();
        tokio::spawn(async move {
            match TcpListener::bind(&listen_addr).await {
                Ok(listener) => {
                    tracing::info!(%listen_addr, "event relay listener started");
                    loop {
                        match listener.accept().await {
                            Ok((stream, _)) => {
                                let sender = local_sender.clone();
                                tokio::spawn(async move {
                                    receive_relay_events(stream, sender).await;
                                });
                            }
                            Err(error) => {
                                tracing::warn!(%error, %listen_addr, "event relay accept failed");
                                sleep(Duration::from_millis(250)).await;
                            }
                        }
                    }
                }
                Err(error) => tracing::error!(%error, %listen_addr, "event relay listener failed to bind"),
            }
        });

        let peer_senders = peers
            .into_iter()
            .map(|peer| {
                let (sender, receiver) = mpsc::channel(RELAY_QUEUE_CAPACITY);
                tokio::spawn(connect_relay_peer(peer.clone(), receiver));
                (peer, sender)
            })
            .collect::<Vec<_>>();
        tokio::spawn(async move {
            while let Some(event) = relay_receiver.recv().await {
                for (peer, sender) in &peer_senders {
                    if sender.try_send(event.clone()).is_err() {
                        tracing::warn!(%peer, "dropping live event because relay peer queue is full or closed");
                    }
                }
            }
        });
        self.relay_sender = Some(relay_sender);
        self
    }

    fn relay(&self, event: AgentEvent) {
        if let Some(sender) = &self.relay_sender {
            if sender.try_send(event).is_err() {
                tracing::warn!("dropping live event because relay queue is full or closed");
            }
        }
    }

    pub async fn publish(&self, event: AgentEvent) {
        // Session conversation/activity events are transient. Only installer status
        // is persisted; chat context is owned by the Agent's native session.
        if !matches!(
            &event,
            AgentEvent::InstallOutput { .. } | AgentEvent::InstallCompleted { .. }
        ) {
            let _ = self.sender.send(event.clone());
            self.relay(event);
            return;
        }
        let Ok(payload) = serde_json::to_string(&event) else {
            tracing::error!("failed to serialize agent event for persistence");
            return;
        };
        let session_id = event.session_id().map(str::to_owned);
        let event_type = serde_json::from_str::<serde_json::Value>(&payload)
            .ok()
            .and_then(|value| {
                value
                    .get("type")
                    .and_then(|value| value.as_str())
                    .map(str::to_owned)
            })
            .unwrap_or_default();
        // Reserve capacity before broadcasting. When SQLite falls behind, this
        // applies backpressure to the producer instead of growing memory without bound.
        let permit = match self.persist_sender.reserve().await {
            Ok(permit) => permit,
            Err(_) => {
                let _ = self.sender.send(event);
                tracing::error!("agent event persistence worker is unavailable");
                return;
            }
        };
        let _ = self.sender.send(event.clone());
        self.relay(event);
        permit.send(PersistedEvent {
            session_id,
            worker_id: self.worker_id.clone(),
            event_type,
            payload,
            created_at: Utc::now().to_rfc3339(),
        });
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AgentEvent> {
        self.sender.subscribe()
    }
}



async fn receive_relay_events(stream: TcpStream, sender: broadcast::Sender<AgentEvent>) {
    let mut framed = Framed::new(stream, LinesCodec::new_with_max_length(MAX_RELAY_EVENT_BYTES));
    while let Some(result) = framed.next().await {
        match result {
            Ok(line) => match serde_json::from_str::<AgentEvent>(&line) {
                Ok(event) => { let _ = sender.send(event); }
                Err(error) => tracing::warn!(%error, "ignoring invalid event relay payload"),
            },
            Err(error) => {
                tracing::warn!(%error, "event relay connection closed after invalid or oversized frame");
                break;
            }
        }
    }
}

async fn connect_relay_peer(peer: String, mut receiver: mpsc::Receiver<AgentEvent>) {
    loop {
        match TcpStream::connect(&peer).await {
            Ok(stream) => {
                tracing::info!(%peer, "connected to event relay peer");
                let mut framed = Framed::new(stream, LinesCodec::new_with_max_length(MAX_RELAY_EVENT_BYTES));
                while let Some(event) = receiver.recv().await {
                    let payload = match serde_json::to_string(&event) {
                        Ok(payload) => payload,
                        Err(error) => {
                            tracing::warn!(%error, %peer, "failed to serialize event relay payload");
                            continue;
                        }
                    };
                    if let Err(error) = framed.send(payload).await {
                        tracing::warn!(%error, %peer, "event relay write failed; reconnecting");
                        break;
                    }
                }
            }
            Err(error) => tracing::debug!(%error, %peer, "event relay peer unavailable; retrying"),
        }
        sleep(Duration::from_secs(1)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    #[test]
    fn retries_only_sqlite_busy_and_locked_codes() {
        for code in ["5", "6", "261", "262"] {
            assert!(
                is_retryable_sqlite_code(Some(code)),
                "expected retryable code {code}"
            );
        }
        for code in ["1", "19", "SQLITE_BUSY", ""] {
            assert!(
                !is_retryable_sqlite_code(Some(code)),
                "unexpected retryable code {code}"
            );
        }
        assert!(!is_retryable_sqlite_code(None));
    }

    async fn bus() -> (EventBus, SqlitePool) {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .idle_timeout(None)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("CREATE TABLE sessions(id TEXT PRIMARY KEY)")
            .execute(&db)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE agent_events(id INTEGER PRIMARY KEY AUTOINCREMENT,session_id TEXT,worker_id TEXT,event_type TEXT NOT NULL,payload TEXT NOT NULL,created_at TEXT NOT NULL)").execute(&db).await.unwrap();
        (EventBus::new(db.clone()), db)
    }

    async fn rows(db: &SqlitePool) -> Vec<(Option<String>, String)> {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        sqlx::query_as("SELECT session_id,event_type FROM agent_events ORDER BY id")
            .fetch_all(db)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn persists_only_install_events_and_keeps_session_events_transient() {
        let (bus, db) = bus().await;
        bus.publish(AgentEvent::InstallOutput {
            agent_id: "pi".into(),
            text: "Installing pi …".into(),
        })
        .await;
        bus.publish(AgentEvent::InstallCompleted {
            agent_id: "pi".into(),
            success: true,
        })
        .await;
        bus.publish(AgentEvent::MessageDelta {
            session_id: "gone".into(),
            text: "x".into(),
        })
        .await;
        assert_eq!(
            rows(&db).await,
            [
                (None, "install.output".to_owned()),
                (None, "install.completed".to_owned())
            ]
        );

        sqlx::query("INSERT INTO sessions(id) VALUES('live')")
            .execute(&db)
            .await
            .unwrap();
        bus.publish(AgentEvent::MessageStarted {
            session_id: "live".into(),
        })
        .await;
        bus.publish(AgentEvent::MessageDelta {
            session_id: "live".into(),
            text: "x".into(),
        })
        .await;
        bus.publish(AgentEvent::MessageCompleted {
            session_id: "live".into(),
        })
        .await;
        assert_eq!(
            rows(&db).await,
            [
                (None, "install.output".to_owned()),
                (None, "install.completed".to_owned()),
            ]
        );
    }
}
