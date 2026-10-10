mod agent_config;
mod agents;
mod events_websocket;
mod filesystem;
mod health;
mod runtime;
mod session_events;
mod session_messages;
mod session_websocket;
mod sessions;
mod workspace;

pub use agent_config::{get_agent_config, save_agent_config};
pub use agents::{agent_models, agent_status, create_agent, list_agents};
pub use events_websocket::ws_events as ws_install_events;
pub use filesystem::tree as filesystem_tree;
pub use health::health;
pub(crate) use runtime::build_agent_config;
pub use runtime::{get_runtime_settings, install_agent, update_runtime_settings};
pub use session_events::list_session_activity;
pub use session_messages::{interrupt, list_messages, send_message};
pub use session_websocket::ws_events;
pub(crate) use sessions::load_session;
pub use sessions::{
    create_session, delete_session, get_session, list_sessions, rename_session, set_session_agent,
    set_session_model, toggle_pin, Session,
};
pub use workspace::{workspace_diff, workspace_file, workspace_files};
