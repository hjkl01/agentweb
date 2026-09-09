use super::adapter::AgentConfig;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{env, fs, path::PathBuf, process::Stdio};
use tokio::process::Command;

#[derive(Debug, Clone, Serialize)]
pub struct AgentModel { pub id: String, pub name: String, pub provider: Option<String>, pub source: String }

fn command_parts(command: &str) -> Result<(String, Vec<String>)> { let mut parts=command.split_whitespace(); let program=parts.next().ok_or_else(|| anyhow::anyhow!("empty agent command"))?.to_owned(); Ok((program,parts.map(str::to_owned).collect())) }
async fn command_output(config:&AgentConfig,extra:&[&str])->Result<String>{let(program,base)=command_parts(&config.command)?;let mut command=Command::new(program);command.args(base).args(extra).stdout(Stdio::piped()).stderr(Stdio::piped());if let Some(dir)=&config.working_directory{command.current_dir(dir);}if let Some(runtime)=&config.runtime_path{command.env("PATH",format!("{}:{}",runtime,env::var("PATH").unwrap_or_default()));}let output=command.output().await?;if !output.status.success(){return Err(anyhow::anyhow!("model discovery command failed: {}",String::from_utf8_lossy(&output.stderr).trim()));}Ok(String::from_utf8_lossy(&output.stdout).to_string())}

fn clean_cli_line(line:&str)->String{let mut result=String::with_capacity(line.len());let mut chars=line.chars().peekable();while let Some(ch)=chars.next(){if ch=='\u{1b}'{if chars.peek()==Some(&'['){chars.next();while let Some(next)=chars.next(){if ('@'..='~').contains(&next){break;}}}continue;}result.push(ch);}result.trim().to_owned()}

fn parse_pi_models(output:&str)->Vec<AgentModel>{let mut result=Vec::new();for raw in output.lines(){let line=clean_cli_line(raw);if line.is_empty()||line.starts_with("Provider")||line.starts_with("Model")||line.starts_with('─'){continue;}let mut columns=line.split_whitespace();let Some(provider)=columns.next()else{continue};let Some(model)=columns.next()else{continue};if provider.starts_with('-')||model.starts_with('-')||provider.eq_ignore_ascii_case("provider"){continue;}let id=if model.contains('/') {model.to_owned()}else{format!("{provider}/{model}")};if result.iter().any(|m:&AgentModel|m.id==id){continue;}result.push(AgentModel{id,name:model.to_owned(),provider:Some(provider.to_owned()),source:"pi --list-models".into()});}result}

fn pi_config_paths()->Vec<PathBuf>{let home=env::var_os("HOME").map(PathBuf::from).unwrap_or_else(||PathBuf::from("/data"));let agent_dir=env::var_os("PI_CODING_AGENT_DIR").map(PathBuf::from).unwrap_or_else(||home.join(".pi/agent"));vec![agent_dir.join("models.json")]}

fn strip_jsonc(text:&str)->String{let mut out=String::with_capacity(text.len());let mut chars=text.chars().peekable();let mut string=false;let mut escaped=false;while let Some(ch)=chars.next(){if string{out.push(ch);if escaped{escaped=false}else if ch=='\\'{escaped=true}else if ch=='"'{string=false}continue;}if ch=='"'{string=true;out.push(ch);continue;}if ch=='/'&&chars.peek()==Some(&'/'){chars.next();while let Some(next)=chars.next(){if next=='\n'{out.push('\n');break;}}continue;}if ch=='/'&&chars.peek()==Some(&'*'){chars.next();while let Some(next)=chars.next(){if next=='*'&&chars.peek()==Some(&'/'){chars.next();break;}}continue;}out.push(ch);}let chars:Vec<char>=out.chars().collect();let mut result=String::with_capacity(out.len());let mut in_string=false;let mut escaped=false;for i in 0..chars.len(){let ch=chars[i];if ch=='"'&&!escaped{in_string=!in_string;}if ch==','&&!in_string{let mut j=i+1;while j<chars.len()&&chars[j].is_whitespace(){j+=1;}if j<chars.len()&&matches!(chars[j],'}'|']'){continue;}}result.push(ch);escaped=ch=='\\'&&!escaped;if ch!='\\'{escaped=false;}}result}

#[derive(Debug,Deserialize)]struct PiModelsFile{providers:Option<std::collections::HashMap<String,PiProvider>>}
#[derive(Debug,Deserialize)]struct PiProvider{models:Option<Vec<PiModel>>}
#[derive(Debug,Deserialize)]struct PiModel{id:Option<String>,name:Option<String>}

fn pi_config_models()->Vec<AgentModel>{let mut result=Vec::new();for path in pi_config_paths(){let Ok(text)=fs::read_to_string(&path)else{continue};let Ok(config)=serde_json::from_str::<PiModelsFile>(&strip_jsonc(&text))else{continue};for(provider,entry)in config.providers.unwrap_or_default(){for model in entry.models.unwrap_or_default(){let Some(id)=model.id.filter(|v|!v.trim().is_empty())else{continue};let full_id=format!("{provider}/{id}");if result.iter().any(|m:&AgentModel|m.id==full_id){continue;}result.push(AgentModel{id:full_id,name:model.name.unwrap_or_else(||id.clone()),provider:Some(provider.clone()),source:path.to_string_lossy().into_owned()});}}}result}
fn merge_models(mut configured:Vec<AgentModel>,discovered:Vec<AgentModel>)->Vec<AgentModel>{for model in discovered{if !configured.iter().any(|item|item.id==model.id){configured.push(model);}}configured}

fn codex_config_paths()->Vec<PathBuf>{let mut paths=Vec::new();if let Some(path)=env::var_os("CODEX_HOME"){paths.push(PathBuf::from(path).join("config.toml"));}if let Some(home)=env::var_os("HOME"){paths.push(PathBuf::from(home).join(".codex/config.toml"));}paths.sort();paths.dedup();paths}
fn codex_config_models()->Vec<AgentModel>{let mut result=Vec::new();for path in codex_config_paths(){let Ok(text)=fs::read_to_string(&path)else{continue};for line in text.lines(){let trimmed=line.trim();if trimmed.starts_with('#')||!trimmed.starts_with("model")||!trimmed.contains('='){continue;}let Some((key,value))=trimmed.split_once('=')else{continue};if key.trim()!="model"{continue;}let value=value.split('#').next().unwrap_or(value).trim().trim_matches('"').trim_matches('\'');if value.is_empty()||result.iter().any(|m:&AgentModel|m.id==value){continue;}result.push(AgentModel{id:value.to_owned(),name:value.to_owned(),provider:Some("openai".into()),source:path.to_string_lossy().into_owned()});}}result}

pub async fn discover(kind:&str,config:&AgentConfig)->Result<Vec<AgentModel>>{match kind{"codex"=>Ok(codex_config_models()),"pi"=>{let configured=pi_config_models();let discovered=command_output(config,&["--list-models"]).await.map(|output|parse_pi_models(&output)).unwrap_or_default();Ok(merge_models(configured,discovered))},_=>Ok(Vec::new())}}
