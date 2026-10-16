mod agent_config;
mod agents;
mod events_websocket;
mod filesystem;
mod health;
mod runtime;
mod scheduled_tasks;
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
pub use scheduled_tasks::{
    create as create_scheduled_task, delete as delete_scheduled_task,
    delete_run as delete_scheduled_task_run, disable as disable_scheduled_task,
    enable as enable_scheduled_task, get as get_scheduled_task, history as scheduled_task_history,
    list as list_scheduled_tasks, run_now as run_scheduled_task_now,
    update as update_scheduled_task,
};
pub use session_events::list_session_activity;
pub use session_messages::{interrupt, list_messages, send_message};
pub use session_websocket::ws_events;
pub(crate) use sessions::load_session;
pub use sessions::{
    create_pi_fork, create_session, delete_session, get_session, list_sessions, list_workspaces,
    rename_session, restart_missing_session, set_session_agent, set_session_model, toggle_pin,
    Session,
};
pub use workspace::{workspace_diff, workspace_file, workspace_files};
