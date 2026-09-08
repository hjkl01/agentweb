#![allow(dead_code)]
use utoipa::OpenApi;
#[derive(OpenApi)]
#[openapi(info(title="Agent Web API",version="0.1.0",description="Agent Web API"),paths(health,auth_login,auth_me,auth_logout,auth_password,auth_sessions,auth_revoke_all,agents_get,agents_post,agent_catalog,runtime_settings_get,runtime_settings_put,node_versions,node_install,agent_status,agent_models,agent_install,sessions_get,sessions_post,session_get,session_delete,session_model,session_messages_get,session_messages_post,session_interrupt,session_files,session_file,session_diff,session_events))]
pub struct ApiDoc;
#[utoipa::path(get,path="/api/health",tag="System",responses((status=200,description="Health")))]fn health(){}
#[utoipa::path(post,path="/api/auth/login",tag="Auth",request_body=serde_json::Value,responses((status=200,description="Login"),(status=401,description="Invalid credentials"),(status=429,description="IP locked for 30 minutes")))]fn auth_login(){}
#[utoipa::path(get,path="/api/auth/me",tag="Auth",responses((status=200,description="Current user"),(status=401,description="Not authenticated")))]fn auth_me(){}
#[utoipa::path(post,path="/api/auth/logout",tag="Auth",responses((status=200,description="Logout")))]fn auth_logout(){}
#[utoipa::path(put,path="/api/auth/password",tag="Auth",request_body=serde_json::Value,responses((status=200,description="Change password")))]fn auth_password(){}
#[utoipa::path(get,path="/api/auth/sessions",tag="Auth",responses((status=200,description="Active sessions")))]fn auth_sessions(){}
#[utoipa::path(post,path="/api/auth/sessions/revoke-all",tag="Auth",responses((status=200,description="Revoke other sessions")))]fn auth_revoke_all(){}
#[utoipa::path(get,path="/api/agents",tag="Agents",responses((status=200,description="List agents")))]fn agents_get(){}
#[utoipa::path(post,path="/api/agents",tag="Agents",request_body=serde_json::Value,responses((status=200,description="Create agent")))]fn agents_post(){}
#[utoipa::path(get,path="/api/agent-catalog",tag="Agents",responses((status=200,description="Agent catalog")))]fn agent_catalog(){}
#[utoipa::path(get,path="/api/runtime/settings",tag="Runtime",responses((status=200,description="Runtime settings")))]fn runtime_settings_get(){}
#[utoipa::path(put,path="/api/runtime/settings",tag="Runtime",request_body=serde_json::Value,responses((status=200,description="Update settings")))]fn runtime_settings_put(){}
#[utoipa::path(get,path="/api/node/versions",tag="Runtime",responses((status=200,description="Node versions")))]fn node_versions(){}
#[utoipa::path(post,path="/api/node/install",tag="Runtime",request_body=serde_json::Value,responses((status=200,description="Install Node")))]fn node_install(){}
#[utoipa::path(get,path="/api/agents/{id}/status",tag="Agents",params(("id"=String,Path)),responses((status=200,description="Agent status")))]fn agent_status(){}
#[utoipa::path(get,path="/api/agents/{id}/models",tag="Models",params(("id"=String,Path)),responses((status=200,description="Agent models")))]fn agent_models(){}
#[utoipa::path(post,path="/api/agents/{id}/install",tag="Agents",params(("id"=String,Path)),responses((status=200,description="Install agent")))]fn agent_install(){}
#[utoipa::path(get,path="/api/sessions",tag="Sessions",responses((status=200,description="List sessions")))]fn sessions_get(){}
#[utoipa::path(post,path="/api/sessions",tag="Sessions",request_body=serde_json::Value,responses((status=200,description="Create session")))]fn sessions_post(){}
#[utoipa::path(get,path="/api/sessions/{id}",tag="Sessions",params(("id"=String,Path)),responses((status=200,description="Get session")))]fn session_get(){}
#[utoipa::path(delete,path="/api/sessions/{id}",tag="Sessions",params(("id"=String,Path)),responses((status=204,description="Delete session")))]fn session_delete(){}
#[utoipa::path(put,path="/api/sessions/{id}/model",tag="Models",params(("id"=String,Path)),request_body=serde_json::Value,responses((status=200,description="Set model")))]fn session_model(){}
#[utoipa::path(get,path="/api/sessions/{id}/messages",tag="Messages",params(("id"=String,Path)))]fn session_messages_get(){}
#[utoipa::path(post,path="/api/sessions/{id}/messages",tag="Messages",params(("id"=String,Path)),request_body=serde_json::Value)]fn session_messages_post(){}
#[utoipa::path(post,path="/api/sessions/{id}/interrupt",tag="Sessions",params(("id"=String,Path)))]fn session_interrupt(){}
#[utoipa::path(get,path="/api/sessions/{id}/files",tag="Workspace",params(("id"=String,Path)))]fn session_files(){}
#[utoipa::path(get,path="/api/sessions/{id}/file/{path}",tag="Workspace",params(("id"=String,Path)))]fn session_file(){}
#[utoipa::path(get,path="/api/sessions/{id}/diff",tag="Workspace",params(("id"=String,Path)))]fn session_diff(){}
#[utoipa::path(get,path="/api/sessions/{id}/events",tag="Events",params(("id"=String,Path)))]fn session_events(){}
