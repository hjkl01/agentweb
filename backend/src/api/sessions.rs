use crate::state::AppState;
use axum::{extract::{Path, State}, http::StatusCode, Json};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::path::{Component, Path as FsPath};
use tokio::fs;
use uuid::Uuid;

#[derive(Serialize, Clone)]
pub struct Session { pub id:String, pub agent_id:String, pub title:String, pub workspace:String, pub status:String, pub native_session_id:Option<String>, pub model:Option<String> }

pub(crate) async fn load_session(db:&sqlx::SqlitePool,id:&str)->Result<Session,StatusCode>{
 let row=sqlx::query("SELECT id,agent_id,title,workspace,status,native_session_id,model FROM sessions WHERE id=?").bind(id).fetch_optional(db).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?.ok_or(StatusCode::NOT_FOUND)?;
 Ok(Session{id:row.get(0),agent_id:row.get(1),title:row.get(2),workspace:row.get(3),status:row.get(4),native_session_id:row.get(5),model:row.get(6)})
}

pub async fn list_sessions(State(s):State<AppState>)->Json<Vec<Session>>{let rows=sqlx::query("SELECT id,agent_id,title,workspace,status,native_session_id,model FROM sessions ORDER BY updated_at DESC").fetch_all(&s.db).await.unwrap_or_default();Json(rows.into_iter().map(|r|Session{id:r.get(0),agent_id:r.get(1),title:r.get(2),workspace:r.get(3),status:r.get(4),native_session_id:r.get(5),model:r.get(6)}).collect())}

#[derive(Deserialize)] pub struct CreateSession { pub agent_id:String,pub title:Option<String>,pub workspace:Option<String>,pub model:Option<String> }

async fn create_workspace(session_id:&str,requested:Option<&str>)->Result<String,StatusCode>{let base_path=std::env::var("AGENTWEB_WORKSPACE_DIR").unwrap_or_else(|_|"./workspaces".into());fs::create_dir_all(&base_path).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;let base=fs::canonicalize(&base_path).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;let name=requested.unwrap_or("").trim();let relative=if name.is_empty()||name=="."{session_id.to_owned()}else{name.to_owned()};let path=FsPath::new(&relative);if path.is_absolute()||path.components().any(|c|matches!(c,Component::ParentDir)){return Err(StatusCode::BAD_REQUEST)}let workspace=base.join(path);fs::create_dir_all(&workspace).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;let canonical=fs::canonicalize(&workspace).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;if !canonical.starts_with(&base){return Err(StatusCode::FORBIDDEN)}Ok(canonical.to_string_lossy().into_owned())}

pub async fn create_session(State(s):State<AppState>,Json(v):Json<CreateSession>)->Result<Json<Session>,StatusCode>{let exists=sqlx::query("SELECT 1 FROM agents WHERE id=?").bind(&v.agent_id).fetch_optional(&s.db).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?.is_some();if !exists{return Err(StatusCode::NOT_FOUND)}let id=Uuid::new_v4().to_string();let workspace=create_workspace(&id,v.workspace.as_deref()).await?;let now=Utc::now().to_rfc3339();let title=v.title.filter(|t|!t.trim().is_empty()).unwrap_or_else(||"New Chat".into());sqlx::query("INSERT INTO sessions(id,agent_id,title,workspace,status,native_session_id,model,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?)").bind(&id).bind(&v.agent_id).bind(&title).bind(&workspace).bind("idle").bind::<Option<String>>(None).bind(&v.model).bind(&now).bind(&now).execute(&s.db).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;Ok(Json(Session{id,agent_id:v.agent_id,title,workspace,status:"idle".into(),native_session_id:None,model:v.model}))}

pub async fn get_session(Path(id):Path<String>,State(s):State<AppState>)->Result<Json<Session>,StatusCode>{Ok(Json(load_session(&s.db,&id).await?))}

async fn remove_default_workspace(id:&str,path:&str){let base_path=std::env::var("AGENTWEB_WORKSPACE_DIR").unwrap_or_else(|_|"./workspaces".into());let Ok(base)=fs::canonicalize(base_path).await else{return};let expected=base.join(id);let Ok(workspace)=fs::canonicalize(path).await else{return};if workspace==expected{let _=fs::remove_dir_all(workspace).await;}}

pub async fn delete_session(Path(id):Path<String>,State(s):State<AppState>)->StatusCode{let session=match load_session(&s.db,&id).await{Ok(v)=>v,Err(e)=>return e};if session.status=="running"{let _=s.agents.interrupt(&id).await;}match sqlx::query("DELETE FROM sessions WHERE id=?").bind(&id).execute(&s.db).await{Ok(r) if r.rows_affected()==1=>{remove_default_workspace(&id,&session.workspace).await;StatusCode::NO_CONTENT},Ok(_)=>StatusCode::NOT_FOUND,Err(_)=>StatusCode::INTERNAL_SERVER_ERROR}}

#[derive(Deserialize)] pub struct SetModel { pub model:Option<String> }
pub async fn set_session_model(Path(id):Path<String>,State(s):State<AppState>,Json(v):Json<SetModel>)->Result<Json<Session>,StatusCode>{let session=load_session(&s.db,&id).await?;if session.status=="running"{return Err(StatusCode::CONFLICT)}sqlx::query("UPDATE sessions SET model=?,updated_at=? WHERE id=?").bind(&v.model).bind(Utc::now().to_rfc3339()).bind(&id).execute(&s.db).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;Ok(Json(load_session(&s.db,&id).await?))}
