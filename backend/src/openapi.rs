use utoipa::OpenApi;

/// Agent Web HTTP API documentation.
#[derive(OpenApi)]
#[openapi(
    info(title = "Agent Web API", version = "0.1.0", description = "Web API for Agent Web: agent runtimes, models, sessions, workspace files, diffs and streaming events."),
    paths(health, auth_login, agents_get, agents_post, agent_catalog, runtime_settings_get, runtime_settings_put, node_versions, node_install, agent_status, agent_models, agent_install, sessions_get, sessions_post, session_get, session_delete, session_model, session_messages_get, session_messages_post, session_interrupt, session_files, session_file, session_diff, session_events)
)]
pub struct ApiDoc;

#[utoipa::path(get, path = "/api/health", tag = "System", responses((status = 200, description = "Service health")))] fn health() {}
#[utoipa::path(post, path = "/api/auth/login", tag = "System", request_body = serde_json::Value, responses((status = 200, description = "Set authentication cookie"), (status = 401, description = "Invalid credentials")))] fn auth_login() {}
#[utoipa::path(get, path = "/api/agents", tag = "Agents", responses((status = 200, description = "List agents")))] fn agents_get() {}
#[utoipa::path(post, path = "/api/agents", tag = "Agents", request_body = serde_json::Value, responses((status = 200, description = "Create agent")))] fn agents_post() {}
#[utoipa::path(get, path = "/api/agent-catalog", tag = "Agents", responses((status = 200, description = "Available agent catalog")))] fn agent_catalog() {}
#[utoipa::path(get, path = "/api/runtime/settings", tag = "Runtime", responses((status = 200, description = "Runtime settings")))] fn runtime_settings_get() {}
#[utoipa::path(put, path = "/api/runtime/settings", tag = "Runtime", request_body = serde_json::Value, responses((status = 200, description = "Updated runtime settings")))] fn runtime_settings_put() {}
#[utoipa::path(get, path = "/api/node/versions", tag = "Runtime", responses((status = 200, description = "Available Node.js versions")))] fn node_versions() {}
#[utoipa::path(post, path = "/api/node/install", tag = "Runtime", request_body = serde_json::Value, responses((status = 200, description = "Node.js installation started/completed")))] fn node_install() {}
#[utoipa::path(get, path = "/api/agents/{id}/status", tag = "Agents", params(("id" = String, Path, description = "Agent id")), responses((status = 200, description = "Agent status")))] fn agent_status() {}
#[utoipa::path(get, path = "/api/agents/{id}/models", tag = "Models", params(("id" = String, Path, description = "Agent id")), responses((status = 200, description = "Models discovered using the selected Agent's native configuration/CLI")))] fn agent_models() {}
#[utoipa::path(post, path = "/api/agents/{id}/install", tag = "Agents", params(("id" = String, Path, description = "Agent id")), responses((status = 200, description = "Agent installation started")))] fn agent_install() {}
#[utoipa::path(get, path = "/api/sessions", tag = "Sessions", responses((status = 200, description = "List sessions")))] fn sessions_get() {}
#[utoipa::path(post, path = "/api/sessions", tag = "Sessions", request_body = serde_json::Value, responses((status = 200, description = "Create session")))] fn sessions_post() {}
#[utoipa::path(get, path = "/api/sessions/{id}", tag = "Sessions", params(("id" = String, Path, description = "Session id")), responses((status = 200, description = "Get session")))] fn session_get() {}
#[utoipa::path(delete, path = "/api/sessions/{id}", tag = "Sessions", params(("id" = String, Path, description = "Session id")), responses((status = 204, description = "Session deleted")))] fn session_delete() {}
#[utoipa::path(put, path = "/api/sessions/{id}/model", tag = "Models", params(("id" = String, Path, description = "Session id")), request_body = serde_json::Value, responses((status = 200, description = "Session model updated")))] fn session_model() {}
#[utoipa::path(get, path = "/api/sessions/{id}/messages", tag = "Messages", params(("id" = String, Path, description = "Session id")), responses((status = 200, description = "Session messages")))] fn session_messages_get() {}
#[utoipa::path(post, path = "/api/sessions/{id}/messages", tag = "Messages", params(("id" = String, Path, description = "Session id")), request_body = serde_json::Value, responses((status = 200, description = "Message execution started")))] fn session_messages_post() {}
#[utoipa::path(post, path = "/api/sessions/{id}/interrupt", tag = "Sessions", params(("id" = String, Path, description = "Session id")), responses((status = 200, description = "Agent execution interrupted")))] fn session_interrupt() {}
#[utoipa::path(get, path = "/api/sessions/{id}/files", tag = "Workspace", params(("id" = String, Path, description = "Session id")), responses((status = 200, description = "Workspace file tree")))] fn session_files() {}
#[utoipa::path(get, path = "/api/sessions/{id}/file/{path}", tag = "Workspace", params(("id" = String, Path, description = "Session id"), ("path" = String, Path, description = "Workspace-relative file path")), responses((status = 200, description = "File content")))] fn session_file() {}
#[utoipa::path(get, path = "/api/sessions/{id}/diff", tag = "Workspace", params(("id" = String, Path, description = "Session id")), responses((status = 200, description = "Git diff and status")))] fn session_diff() {}
#[utoipa::path(get, path = "/api/sessions/{id}/events", tag = "Events", params(("id" = String, Path, description = "Session id")), responses((status = 101, description = "WebSocket event stream")))] fn session_events() {}
