use crate::{agents::AgentConfig, events::AgentEvent, state::AppState};
use axum::{extract::{Path, State, WebSocketUpgrade}, response::IntoResponse, Json};
use axum::extract::ws::{Message, WebSocket};
use chrono::Utc;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use tokio::process::Command;
use uuid::Uuid;

#[derive(Serialize)] pub struct Health { pub status: &'static str }
pub async fn health() -> Json<Health> { Json(Health { status: "ok" }) }

#[derive(Serialize)] pub struct Agent { pub id:String, pub name:String, pub kind:String, pub command:String, pub working_directory:Option<String>, pub installed:bool, pub version:Option<String> }
#[derive(Serialize)] pub struct CatalogItem { pub id:&'static str, pub name:&'static str, pub description:&'static str, pub installed:bool, pub requirements:Vec<&'static str> }

pub async fn catalog(State(state): State<AppState>) -> Json<Vec<CatalogItem>> {
    let codex_installed = Command::new("codex").arg("--version").output().await.is_ok();
    Json(vec![
        CatalogItem { id:"codex", name:"Codex", description:"OpenAI coding agent", installed:codex_installed, requirements:vec![] },
        CatalogItem { id:"claude-code", name:"Claude Code", description:"Anthropic coding agent", installed:false, requirements:vec!["Node.js"] },
        CatalogItem { id:"opencode", name:"OpenCode", description:"Open-source coding agent", installed:false, requirements:vec!["Node.js"] },
        CatalogItem { id:"openclaw", name:"OpenClaw", description:"General purpose agent", installed:false, requirements:vec![] },
    ])
}

#[derive(Deserialize)] pub struct CreateAgent { pub name:String, pub kind:String, pub command:String, pub working_directory:Option<String> }
pub async fn list_agents(State(s):State<AppState>) -> Json<Vec<Agent>> {
    let rows=sqlx::query("SELECT id,name,kind,command,working_directory,installed,version FROM agents ORDER BY name").fetch_all(&s.db).await.unwrap_or_default();
    Json(rows.into_iter().map(|r| Agent{id:r.get(0),name:r.get(1),kind:r.get(2),command:r.get(3),working_directory:r.get(4),installed:r.get::<i64,_>(5)!=0,version:r.get(6)}).collect())
}
pub async fn create_agent(State(s):State<AppState>,Json(v):Json<CreateAgent>)->Json<Agent>{
    let id=Uuid::new_v4().to_string(); let now=Utc::now().to_rfc3339();
    let _=sqlx::query("INSERT INTO agents(id,name,kind,command,working_directory,installed,created_at,updated_at) VALUES(?,?,?,?,?,0,?,?)").bind(&id).bind(&v.name).bind(&v.kind).bind(&v.command).bind(&v.working_directory).bind(&now).bind(&now).execute(&s.db).await;
    Json(Agent{id,name:v.name,kind:v.kind,command:v.command,working_directory:v.working_directory,installed:false,version:None})
}
pub async fn agent_status(Path(id):Path<String>,State(s):State<AppState>)->Json<serde_json::Value>{
    if id=="codex" { let out=Command::new("codex").arg("--version").output().await; return Json(serde_json::json!({"id":id,"installed":out.is_ok(),"version":out.ok().map(|o|String::from_utf8_lossy(&o.stdout).trim().to_string())})); }
    let row=sqlx::query("SELECT installed,version FROM agents WHERE id=?").bind(&id).fetch_optional(&s.db).await.ok().flatten(); Json(serde_json::json!({"id":id,"installed":row.as_ref().map(|r|r.get::<i64,_>(0)!=0).unwrap_or(false),"version":row.and_then(|r|r.get::<Option<String>,_>(1))}))
}
pub async fn install_agent(Path(id):Path<String>,State(s):State<AppState>)->Json<serde_json::Value>{
    let events=s.events.clone(); let id2=id.clone();
    tokio::spawn(async move { let (program,args):(String,Vec<String>)=match id2.as_str(){"claude-code"=>("npm".into(),vec!["install".into(),"-g".into(),"@anthropic-ai/claude-code".into()]),"opencode"=>("npm".into(),vec!["install".into(),"-g".into(),"opencode-ai@latest".into()]),_=>{events.publish(AgentEvent::InstallCompleted{agent_id:id2,success:false});return;}}; let mut c=Command::new(program); c.args(args); match c.output().await {Ok(o)=>{events.publish(AgentEvent::InstallOutput{agent_id:id2.clone(),text:String::from_utf8_lossy(&o.stdout).into_owned()});events.publish(AgentEvent::InstallCompleted{agent_id:id2,success:o.status.success()});},Err(e)=>events.publish(AgentEvent::InstallOutput{agent_id:id2,text:e.to_string()})} }); let _=s; Json(serde_json::json!({"status":"started","agent_id":id}))
}

#[derive(Serialize)] pub struct Session { pub id:String,pub agent_id:String,pub title:String,pub workspace:String,pub status:String }
#[derive(Deserialize)] pub struct CreateSession { pub agent_id:String,pub title:Option<String>,pub workspace:String }
pub async fn list_sessions(State(s):State<AppState>)->Json<Vec<Session>>{let rows=sqlx::query("SELECT id,agent_id,title,workspace,status FROM sessions ORDER BY updated_at DESC").fetch_all(&s.db).await.unwrap_or_default();Json(rows.into_iter().map(|r|Session{id:r.get(0),agent_id:r.get(1),title:r.get(2),workspace:r.get(3),status:r.get(4)}).collect())}
pub async fn create_session(State(s):State<AppState>,Json(v):Json<CreateSession>)->Json<Session>{let id=Uuid::new_v4().to_string();let now=Utc::now().to_rfc3339();let title=v.title.unwrap_or_else(||"New Chat".into());let _=sqlx::query("INSERT INTO sessions VALUES(?,?,?,?,?,?,?)").bind(&id).bind(&v.agent_id).bind(&title).bind(&v.workspace).bind("created").bind(&now).bind(&now).execute(&s.db).await;Json(Session{id,agent_id:v.agent_id,title,workspace:v.workspace,status:"created".into()})}
pub async fn get_session(Path(id):Path<String>,State(s):State<AppState>)->Result<Json<Session>,axum::http::StatusCode>{let r=sqlx::query("SELECT id,agent_id,title,workspace,status FROM sessions WHERE id=?").bind(&id).fetch_optional(&s.db).await.map_err(|_|axum::http::StatusCode::INTERNAL_SERVER_ERROR)?.ok_or(axum::http::StatusCode::NOT_FOUND)?;Ok(Json(Session{id:r.get(0),agent_id:r.get(1),title:r.get(2),workspace:r.get(3),status:r.get(4)}))}
pub async fn delete_session(Path(id):Path<String>,State(s):State<AppState>)->axum::http::StatusCode{let _=sqlx::query("DELETE FROM sessions WHERE id=?").bind(id).execute(&s.db).await;axum::http::StatusCode::NO_CONTENT}
#[derive(Deserialize)] pub struct MessageReq{pub message:String}
pub async fn send_message(Path(id):Path<String>,State(s):State<AppState>,Json(v):Json<MessageReq>)->Json<serde_json::Value>{let row=sqlx::query("SELECT agent_id,workspace FROM sessions WHERE id=?").bind(&id).fetch_optional(&s.db).await.ok().flatten();let Some(r)=row else{return Json(serde_json::json!({"error":"session not found"}))};let agent_id:String=r.get(0);let workspace:Option<String>=r.get(1);let _=sqlx::query("INSERT INTO messages VALUES(?,?,?,?,?)").bind(Uuid::new_v4().to_string()).bind(&id).bind("user").bind(&v.message).bind(Utc::now().to_rfc3339()).execute(&s.db).await;s.events.publish(AgentEvent::SessionStarted{session_id:id.clone()});let cfg=AgentConfig{id:agent_id.clone(),command:if agent_id=="codex"{"codex".into()}else{sqlx::query("SELECT command FROM agents WHERE id=?").bind(&agent_id).fetch_optional(&s.db).await.ok().flatten().map(|r|r.get(0)).unwrap_or_default()},working_directory:workspace};let manager=s.agents.clone();let events=s.events.clone();let msg=v.message;tokio::spawn(async move{let adapter=manager.adapter(&cfg.id).await;if let Err(e)=adapter.send_message(&cfg,&id,&msg,&events).await{events.publish(AgentEvent::Error{session_id:id,message:e.to_string()});}});Json(serde_json::json!({"status":"started"}))}
pub async fn interrupt(Path(id):Path<String>,State(s):State<AppState>)->Json<serde_json::Value>{let _=s.agents.adapter("process").await.interrupt(&id).await;Json(serde_json::json!({"status":"ok"}))}
pub async fn ws_events(Path(_id):Path<String>,ws:WebSocketUpgrade,State(s):State<AppState>)->impl IntoResponse{ws.on_upgrade(move |socket| websocket(socket,s))}
async fn websocket(mut socket:WebSocket,s:AppState){let mut rx=s.events.subscribe();while let Ok(event)=rx.recv().await{if let Ok(text)=serde_json::to_string(&event){if socket.send(Message::Text(text.into())).await.is_err(){break;}}}}
