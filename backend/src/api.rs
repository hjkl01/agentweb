use crate::{agents::{definition, models::AgentModel, AgentConfig}, events::AgentEvent, installation::runtime, state::AppState};
use axum::{extract::{ws::WebSocket, Path, State, WebSocketUpgrade}, http::StatusCode, response::IntoResponse, Json};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::{collections::HashMap, path::{Path as FsPath, PathBuf}, process::Stdio};
use tokio::{fs, process::Command};
use uuid::Uuid;

async fn setting(db: &sqlx::SqlitePool, key: &str) -> Option<String> { sqlx::query("SELECT value FROM runtime_settings WHERE key=?").bind(key).fetch_optional(db).await.ok().flatten().map(|r| r.get(0)) }
async fn save_setting(db: &sqlx::SqlitePool, key: &str, value: &str) -> Result<(), sqlx::Error> { sqlx::query("INSERT INTO runtime_settings(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value").bind(key).bind(value).execute(db).await.map(|_| ()) }
async fn configured_agent_path(db: &sqlx::SqlitePool, id: &str) -> Option<String> { let raw = setting(db, "agent_paths").await?; let paths: HashMap<String,String> = serde_json::from_str(&raw).ok()?; paths.get(id).cloned().filter(|p| !p.trim().is_empty()) }
async fn configured_node_path(db: &sqlx::SqlitePool) -> Option<PathBuf> { setting(db, "node_path").await.filter(|p| !p.trim().is_empty()).map(PathBuf::from) }
async fn node_bin_dir(db: &sqlx::SqlitePool) -> Option<PathBuf> {
    if let Some(p) = configured_node_path(db).await { if p.is_file() { return p.parent().map(PathBuf::from); } if p.join("node").is_file() { return Some(p); } }
    runtime::detect_installed_node().await.ok().flatten().map(|(v,_)| runtime::node_bin(&v))
}
fn agent_executable(id: &str) -> &str { match id { "codex"=>"codex", "claude-code"=>"claude", "opencode"=>"opencode", "pi"=>"pi", "openclaw"=>"openclaw", _=>id } }
async fn resolve_agent_binary(db: &sqlx::SqlitePool, id: &str) -> Option<PathBuf> {
    if let Some(p)=configured_agent_path(db,id).await { let p=FsPath::new(&p); if p.exists(){return Some(p.to_path_buf());} }
    if let Some(node_bin)=node_bin_dir(db).await { let binary=node_bin.join(agent_executable(id)); if binary.exists(){return Some(binary);} }
    let out=Command::new("which").arg(agent_executable(id)).output().await.ok()?;
    out.status.success().then(||PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()))
}
async fn detect_agent_version(binary: &FsPath) -> Option<String> { let out=Command::new(binary).arg("--version").output().await.ok()?; if !out.status.success(){return None;} let v=String::from_utf8_lossy(&out.stdout).trim().trim_start_matches('v').to_owned(); (!v.is_empty()).then_some(v) }
async fn detect_agent(db:&sqlx::SqlitePool,id:&str)->(bool,Option<String>,Option<String>){let b=match resolve_agent_binary(db,id).await{Some(v)=>v,None=>return(false,None,None)};let v=detect_agent_version(&b).await;(v.is_some(),v,Some(b.to_string_lossy().into_owned()))}
async fn agent_runtime_label(id:&str,db:&sqlx::SqlitePool)->Option<String>{if id=="codex"{return None;}if let Ok(Some((v,_)))=runtime::detect_installed_node().await{return Some(format!("Node {v}"));}node_bin_dir(db).await.and_then(|p|p.join("node").exists().then(||format!("Node ({})",p.display())))}

#[derive(Deserialize)] pub struct InstallNodeRequest{pub version:String}
pub async fn install_node(Json(v):Json<InstallNodeRequest>)->Result<Json<serde_json::Value>,StatusCode>{runtime::install_node(&v.version, |_|{}).await.map_err(|_|StatusCode::BAD_REQUEST)?;Ok(Json(serde_json::json!({"status":"installed","version":v.version})))}
#[derive(Serialize)] pub struct CatalogItem{pub id:&'static str,pub name:&'static str,pub description:&'static str,pub installed:bool,pub requirements:Vec<&'static str>}
pub async fn catalog(State(s):State<AppState>)->Json<Vec<CatalogItem>>{let db=&s.db;let c=detect_agent(db,"codex").await;let cl=detect_agent(db,"claude-code").await;let o=detect_agent(db,"opencode").await;let p=detect_agent(db,"pi").await;let oc=detect_agent(db,"openclaw").await;Json(vec![CatalogItem{id:"codex",name:"Codex",description:"OpenAI coding agent",installed:c.0,requirements:vec![]},CatalogItem{id:"claude-code",name:"Claude Code",description:"Anthropic coding agent",installed:cl.0,requirements:vec!["Node.js"]},CatalogItem{id:"opencode",name:"OpenCode",description:"Open-source coding agent",installed:o.0,requirements:vec!["Node.js"]},CatalogItem{id:"pi",name:"Pi",description:"Pi coding agent",installed:p.0,requirements:vec!["Node.js"]},CatalogItem{id:"openclaw",name:"OpenClaw",description:"General purpose agent",installed:oc.0,requirements:vec![]])}
#[derive(Serialize)] pub struct RuntimeSettings{pub node_path:Option<String>,pub agent_paths:HashMap<String,String>}
pub async fn get_runtime_settings(State(s):State<AppState>)->Json<RuntimeSettings>{let paths=setting(&s.db,"agent_paths").await.and_then(|v|serde_json::from_str(&v).ok()).unwrap_or_default();Json(RuntimeSettings{node_path:setting(&s.db,"node_path").await,agent_paths:paths})}
#[derive(Deserialize)] pub struct UpdateRuntimeSettings{pub node_path:Option<String>,pub agent_paths:HashMap<String,String>}
pub async fn update_runtime_settings(State(s):State<AppState>,Json(v):Json<UpdateRuntimeSettings>)->Result<Json<RuntimeSettings>,StatusCode>{let node_path=v.node_path.map(|p|p.trim().to_owned()).filter(|p|!p.is_empty());let paths=v.agent_paths.into_iter().filter_map(|(k,p)|{let p=p.trim().to_owned();(!k.trim().is_empty()&&!p.is_empty()).then_some((k,p))}).collect::<HashMap<_,_>>();save_setting(&s.db,"node_path",node_path.as_deref().unwrap_or("")).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;save_setting(&s.db,"agent_paths",&serde_json::to_string(&paths).map_err(|_|StatusCode::BAD_REQUEST)?).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;Ok(Json(RuntimeSettings{node_path,agent_paths:paths}))}

#[derive(Serialize)] pub struct Agent{pub id:String,pub name:String,pub kind:String,pub command:String,pub working_directory:Option<String>,pub installed:bool,pub version:Option<String>}
pub async fn list_agents(State(s):State<AppState>)->Json<Vec<Agent>>{let rows=sqlx::query("SELECT id,name,kind,command,working_directory,installed,version FROM agents ORDER BY name").fetch_all(&s.db).await.unwrap_or_default();Json(rows.into_iter().map(|r|Agent{id:r.get(0),name:r.get(1),kind:r.get(2),command:r.get(3),working_directory:r.get(4),installed:r.get::<i64,_>(5)!=0,version:r.get(6)}).collect())}
#[derive(Deserialize)] pub struct CreateAgent{pub name:String,pub kind:String,pub command:String,pub working_directory:Option<String>}
pub async fn create_agent(State(s):State<AppState>,Json(v):Json<CreateAgent>)->Json<Agent>{let id=Uuid::new_v4().to_string();let now=Utc::now().to_rfc3339();let _=sqlx::query("INSERT INTO agents(id,name,kind,command,working_directory,installed,created_at,updated_at) VALUES(?,?,?,?,?,0,?,?)").bind(&id).bind(&v.name).bind(&v.kind).bind(&v.command).bind(&v.working_directory).bind(&now).bind(&now).execute(&s.db).await;Json(Agent{id,name:v.name,kind:v.kind,command:v.command,working_directory:v.working_directory,installed:false,version:None})}
#[derive(Serialize)] pub struct AgentStatus{pub id:String,pub installed:bool,pub version:Option<String>,pub path:Option<String>,pub runtime:Option<String>}
pub async fn agent_status(Path(id):Path<String>,State(s):State<AppState>)->Json<AgentStatus>{let(i,v,p)=detect_agent(&s.db,&id).await;let runtime=if i{agent_runtime_label(&id,&s.db).await}else{None};Json(AgentStatus{id,installed:i,version:v,path:p,runtime})}

pub async fn agent_models(Path(id):Path<String>,State(s):State<AppState>)->Result<Json<Vec<AgentModel>>,StatusCode>{
    let (kind,command,working_directory)=if let Some(row)=sqlx::query("SELECT kind,command,working_directory FROM agents WHERE id=?").bind(&id).fetch_optional(&s.db).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?{(row.get::<String,_>(0),row.get::<String,_>(1),row.get::<Option<String>,_>(2))}else if let Some(def)=definition::BUILT_IN_AGENTS.iter().find(|a|a.id==id){(def.kind.to_owned(),def.command.to_owned(),None)}else{return Err(StatusCode::NOT_FOUND)};
    let binary=resolve_agent_binary(&s.db,&id).await.ok_or(StatusCode::NOT_FOUND)?;
    let configured=configured_agent_path(&s.db,&id).await;
    let command=if command.trim().is_empty(){configured.unwrap_or_else(||binary.to_string_lossy().into_owned())}else{command};
    let runtime_path=node_bin_dir(&s.db).await.map(|p|p.to_string_lossy().into_owned());
    let cfg=AgentConfig{id:kind.clone(),command,working_directory,native_session_id:None,runtime_path,model:None};
    Ok(Json(s.agents.adapter(&kind).await.list_models(&cfg).await.map_err(|_|StatusCode::BAD_GATEWAY)?))
}

pub async fn install_agent(Path(id):Path<String>,State(s):State<AppState>)->Json<serde_json::Value>{let events=s.events.clone();let db=s.db.clone();let id2=id.clone();tokio::spawn(async move{let success=install_agent_inner(&db,&id2,&events).await;events.publish(AgentEvent::InstallCompleted{agent_id:id2,success});});Json(serde_json::json!({"status":"started","agent_id":id}))}
async fn install_agent_inner(db:&sqlx::SqlitePool,id:&str,events:&crate::events::EventBus)->bool{
    if id=="openclaw"{events.publish(AgentEvent::InstallOutput{agent_id:id.to_string(),text:"OpenClaw installation package is not configured.".into()});return false;}
    if id=="codex"{
        let Some(node_bin)=node_bin_dir(db).await else { events.publish(AgentEvent::InstallOutput{agent_id:id.to_string(),text:"Please install or configure Node.js first.".into()}); return false; };
        let mut cmd=Command::new(node_bin.join("npm"));
        cmd.args(["install","-g","@openai/codex"]).env("PATH",format!("{}:{}",node_bin.display(),std::env::var("PATH").unwrap_or_default())).stdout(Stdio::piped()).stderr(Stdio::piped());
        if let Some(node_path)=configured_node_path(db).await{if node_path.is_file(){if let Some(home)=node_path.parent().and_then(|x|x.parent()){cmd.env("NPM_CONFIG_PREFIX",home);}}}
        events.publish(AgentEvent::InstallOutput{agent_id:id.to_string(),text:"Installing @openai/codex …".into()});
        let output=match cmd.output().await{Ok(o)=>o,Err(e)=>{events.publish(AgentEvent::InstallOutput{agent_id:id.to_string(),text:format!("npm install failed: {e}")});return false;}};
        events.publish(AgentEvent::InstallOutput{agent_id:id.to_string(),text:format!("{}{}",String::from_utf8_lossy(&output.stdout),String::from_utf8_lossy(&output.stderr))});
        if !output.status.success(){return false;}
        let binary=node_bin.join("codex");
        let version=match detect_agent_version(&binary).await{Some(v)=>v,None=>return false};
        let now=Utc::now().to_rfc3339();
        let _=sqlx::query("UPDATE agents SET installed=1,version=?,updated_at=? WHERE id=?").bind(&version).bind(&now).bind(id).execute(db).await;
        events.publish(AgentEvent::InstallOutput{agent_id:id.to_string(),text:format!("Installed {id} version {version}")});
        return true;
    }
    let node_bin=match node_bin_dir(db).await{Some(p)=>p,None=>{events.publish(AgentEvent::InstallOutput{agent_id:id.to_string(),text:"Please install or configure Node.js first.".into()});return false;}};
    let package=match id{"claude-code"=>"@anthropic-ai/claude-code","opencode"=>"opencode-ai@latest","pi"=>"@mariozechner/pi-coding-agent",_=>{events.publish(AgentEvent::InstallOutput{agent_id:id.to_string(),text:format!("Unknown agent: {id}")});return false;}};
    let mut cmd=Command::new(node_bin.join("npm"));cmd.args(["install","-g",package]).env("PATH",format!("{}:{}",node_bin.display(),std::env::var("PATH").unwrap_or_default())).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(node_path)=configured_node_path(db).await{if node_path.is_file(){if let Some(home)=node_path.parent().and_then(|x|x.parent()){cmd.env("NPM_CONFIG_PREFIX",home);}}}
    events.publish(AgentEvent::InstallOutput{agent_id:id.to_string(),text:format!("Installing {package} …")});let output=match cmd.output().await{Ok(o)=>o,Err(e)=>{events.publish(AgentEvent::InstallOutput{agent_id:id.to_string(),text:format!("npm install failed: {e}")});return false;}};
    events.publish(AgentEvent::InstallOutput{agent_id:id.to_string(),text:format!("{}{}",String::from_utf8_lossy(&output.stdout),String::from_utf8_lossy(&output.stderr))});if !output.status.success(){return false;}
    let binary=node_bin.join(agent_executable(id));let version=match detect_agent_version(&binary).await{Some(v)=>v,None=>return false};let now=Utc::now().to_rfc3339();let _=sqlx::query("UPDATE agents SET installed=1,version=?,updated_at=? WHERE id=?").bind(&version).bind(&now).bind(id).execute(db).await;events.publish(AgentEvent::InstallOutput{agent_id:id.to_string(),text:format!("Installed {id} version {version}")});true
}

#[derive(Serialize)] pub struct Session{pub id:String,pub agent_id:String,pub title:String,pub workspace:String,pub status:String,pub native_session_id:Option<String>,pub model:Option<String>}
#[derive(Deserialize)] pub struct CreateSession{pub agent_id:String,pub title:Option<String>,pub workspace:String,pub model:Option<String>}
pub async fn list_sessions(State(s):State<AppState>)->Json<Vec<Session>>{let rows=sqlx::query("SELECT id,agent_id,title,workspace,status,native_session_id,model FROM sessions ORDER BY updated_at DESC").fetch_all(&s.db).await.unwrap_or_default();Json(rows.into_iter().map(|r|Session{id:r.get(0),agent_id:r.get(1),title:r.get(2),workspace:r.get(3),status:r.get(4),native_session_id:r.get(5),model:r.get(6)}).collect())}
pub async fn create_session(State(s):State<AppState>,Json(v):Json<CreateSession>)->Json<Session>{let id=Uuid::new_v4().to_string();let now=Utc::now().to_rfc3339();let title=v.title.unwrap_or_else(||"New Chat".into());let _=sqlx::query("INSERT INTO sessions(id,agent_id,title,workspace,status,native_session_id,model,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?)").bind(&id).bind(&v.agent_id).bind(&title).bind(&v.workspace).bind("idle").bind::<Option<String>>(None).bind(&v.model).bind(&now).bind(&now).execute(&s.db).await;Json(Session{id,agent_id:v.agent_id,title,workspace:v.workspace,status:"idle".into(),native_session_id:None,model:v.model})}
pub async fn get_session(Path(id):Path<String>,State(s):State<AppState>)->Result<Json<Session>,StatusCode>{let r=sqlx::query("SELECT id,agent_id,title,workspace,status,native_session_id,model FROM sessions WHERE id=?").bind(&id).fetch_optional(&s.db).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?.ok_or(StatusCode::NOT_FOUND)?;Ok(Json(Session{id:r.get(0),agent_id:r.get(1),title:r.get(2),workspace:r.get(3),status:r.get(4),native_session_id:r.get(5),model:r.get(6)}))}
pub async fn delete_session(Path(id):Path<String>,State(s):State<AppState>)->StatusCode{match sqlx::query("DELETE FROM sessions WHERE id=?").bind(id).execute(&s.db).await{Ok(_)=>StatusCode::NO_CONTENT,Err(_)=>StatusCode::INTERNAL_SERVER_ERROR}}
#[derive(Deserialize)] pub struct SetModel{pub model:Option<String>}
pub async fn set_session_model(Path(id):Path<String>,State(s):State<AppState>,Json(v):Json<SetModel>)->Result<Json<Session>,StatusCode>{sqlx::query("UPDATE sessions SET model=?,updated_at=? WHERE id=?").bind(&v.model).bind(Utc::now().to_rfc3339()).bind(&id).execute(&s.db).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;get_session(Path(id),State(s)).await}
pub async fn list_messages(Path(id):Path<String>,State(s):State<AppState>)->Result<Json<Vec<serde_json::Value>>,StatusCode>{let rows=sqlx::query("SELECT id,role,content,created_at FROM messages WHERE session_id=? ORDER BY created_at").bind(id).fetch_all(&s.db).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;Ok(Json(rows.into_iter().map(|r|serde_json::json!({"id":r.get::<String,_>(0),"role":r.get::<String,_>(1),"content":r.get::<String,_>(2),"created_at":r.get::<String,_>(3)})).collect()))}
#[derive(Deserialize)] pub struct SendMessage{pub message:String}
pub async fn send_message(Path(id):Path<String>,State(s):State<AppState>,Json(v):Json<SendMessage>)->Result<Json<serde_json::Value>,StatusCode>{let session=get_session(Path(id.clone()),State(s.clone())).await?.0;let _=sqlx::query("INSERT INTO messages(id,session_id,role,content,created_at) VALUES(?,?,?,?,?)").bind(Uuid::new_v4().to_string()).bind(&id).bind("user").bind(&v.message).bind(Utc::now().to_rfc3339()).execute(&s.db).await;let events=s.events.clone();let db=s.db.clone();let agents=s.agents.clone();tokio::spawn(async move{crate::agents::process::run_session(db,agents,session,v.message,events).await;});Ok(Json(serde_json::json!({"status":"started"})))}
pub async fn interrupt(Path(id):Path<String>,State(s):State<AppState>)->Result<Json<serde_json::Value>,StatusCode>{s.agents.interrupt(&id).await.map_err(|_|StatusCode::BAD_GATEWAY)?;Ok(Json(serde_json::json!({"status":"interrupted"})))}

async fn workspace_root(path: &str) -> Result<PathBuf, StatusCode> { fs::canonicalize(path).await.map_err(|_|StatusCode::NOT_FOUND) }
async fn workspace_file_path(workspace: &str, relative: &str) -> Result<PathBuf, StatusCode> {
    if relative.trim().is_empty() || FsPath::new(relative).is_absolute() { return Err(StatusCode::BAD_REQUEST); }
    let root=workspace_root(workspace).await?;
    let candidate=fs::canonicalize(root.join(relative)).await.map_err(|_|StatusCode::NOT_FOUND)?;
    if !candidate.starts_with(&root) { return Err(StatusCode::FORBIDDEN); }
    Ok(candidate)
}
pub async fn workspace_files(Path(id):Path<String>,State(s):State<AppState>)->Result<Json<Vec<serde_json::Value>>,StatusCode>{let session=get_session(Path(id),State(s.clone())).await?.0;let root=workspace_root(&session.workspace).await?;let mut out=Vec::new();let mut stack=vec![root.clone()];while let Some(dir)=stack.pop(){let mut entries=fs::read_dir(&dir).await.map_err(|_|StatusCode::NOT_FOUND)?;while let Some(entry)=entries.next_entry().await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?{let path=entry.path();let metadata=entry.metadata().await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;if metadata.is_dir(){stack.push(path.clone());}let relative=path.strip_prefix(&root).map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?.to_string_lossy().replace('\\','/');out.push(serde_json::json!({"name":entry.file_name().to_string_lossy(),"path":relative,"kind":if metadata.is_dir(){"directory"}else{"file"},"size":metadata.len()}));}}Ok(Json(out))}
pub async fn workspace_file(Path((id,path)):Path<(String,String)>,State(s):State<AppState>)->Result<Json<serde_json::Value>,StatusCode>{let session=get_session(Path(id),State(s)).await?.0;let file=workspace_file_path(&session.workspace,&path).await?;let content=fs::read_to_string(file).await.map_err(|_|StatusCode::NOT_FOUND)?;Ok(Json(serde_json::json!({"path":path,"content":content})))}
pub async fn workspace_diff(Path(id):Path<String>,State(s):State<AppState>)->Result<Json<serde_json::Value>,StatusCode>{let session=get_session(Path(id),State(s.clone())).await?.0;let output=Command::new("git").arg("-C").arg(&session.workspace).args(["diff","--no-ext-diff"]).output().await.map_err(|_|StatusCode::BAD_GATEWAY)?;Ok(Json(serde_json::json!({"status":"ok","diff":String::from_utf8_lossy(&output.stdout)})))}
pub async fn ws_events(Path(id):Path<String>,ws:WebSocketUpgrade,State(s):State<AppState>)->impl IntoResponse{ws.on_upgrade(move|socket|websocket(socket,s,id))}
fn event_session_id(event:&AgentEvent)->Option<&str>{match event{AgentEvent::SessionStarted{session_id}|AgentEvent::MessageStarted{session_id}|AgentEvent::MessageDelta{session_id,..}|AgentEvent::MessageCompleted{session_id}|AgentEvent::ThinkingStarted{session_id}|AgentEvent::ThinkingDelta{session_id,..}|AgentEvent::ThinkingCompleted{session_id}|AgentEvent::ToolStarted{session_id,..}|AgentEvent::ToolOutput{session_id,..}|AgentEvent::ToolCompleted{session_id,..}|AgentEvent::FileCreated{session_id,..}|AgentEvent::FileModified{session_id,..}|AgentEvent::FileDeleted{session_id,..}|AgentEvent::CommandStarted{session_id,..}|AgentEvent::CommandOutput{session_id,..}|AgentEvent::CommandCompleted{session_id}|AgentEvent::Error{session_id,..}|AgentEvent::SessionCompleted{session_id}=>Some(session_id),AgentEvent::InstallOutput{..}|AgentEvent::InstallCompleted{..}=>None}}
async fn websocket(mut socket:WebSocket,s:AppState,session_id:String){let mut rx=s.events.subscribe();while let Ok(event)=rx.recv().await{if event_session_id(&event)!=Some(session_id.as_str()){continue;}if let Ok(text)=serde_json::to_string(&event){if socket.send(axum::extract::ws::Message::Text(text.into())).await.is_err(){break;}}}}
#[derive(Serialize)] pub struct Health{pub status:&'static str}
pub async fn health()->Json<Health>{Json(Health{status:"ok"})}
