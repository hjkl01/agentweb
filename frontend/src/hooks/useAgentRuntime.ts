import { useCallback, useEffect, useState } from 'react';
import { api } from '../lib/api';
import type { Agent, AgentModel, NodeInfo } from '../types';

type RuntimeSettings={node_path?:string;agent_paths:Record<string,string>;default_agent_id?:string;default_model?:string};
type AgentStatus={id:string;installed:boolean;version?:string;path?:string;runtime?:string};

export function useAgentRuntime(){
 const [agents,setAgents]=useState<Agent[]>([]);const [node,setNode]=useState<NodeInfo>();const [nodeVersion,setNodeVersion]=useState('');const [defaultAgentId,setDefaultAgentId]=useState<string>();const [defaultModel,setDefaultModel]=useState<string>();const [defaultAgentAvailable,setDefaultAgentAvailable]=useState(false);const [error,setError]=useState<string>();
 const refresh=useCallback(async()=>{setError(undefined);try{const [catalog,nodeInfo,settings]=await Promise.all([api<Agent[]>('/agent-catalog'),api<NodeInfo>('/node/versions'),api<RuntimeSettings>('/runtime/settings')]);
   const status=await Promise.all(catalog.map(async agent=>{try{return await api<AgentStatus>(`/agents/${encodeURIComponent(agent.id)}/status`)}catch{return undefined}}));
   const resolved=catalog.map((agent,index)=>{const current=status[index];return current?{...agent,installed:current.installed}:agent});
   setAgents(resolved);setNode(nodeInfo);setNodeVersion(current=>current||nodeInfo.active||'');
   const available=Boolean(settings.default_agent_id&&resolved.some(agent=>agent.id===settings.default_agent_id&&agent.installed));
   setDefaultAgentAvailable(available);setDefaultAgentId(settings.default_agent_id);
   let model=settings.default_model;
   if(available&&settings.default_agent_id&&model){try{const models=await api<AgentModel[]>(`/agents/${encodeURIComponent(settings.default_agent_id)}/models`);if(models.length&&!models.some(item=>item.id===model))model=undefined}catch{model=undefined}}
   setDefaultModel(model);
  }catch(error){const message=error instanceof Error?error.message:String(error);console.error('Failed to load Agent runtime:',error);setError(message)}},[]);
 const saveDefaults=useCallback(async(agentId?:string,model?:string)=>{const current=await api<RuntimeSettings>('/runtime/settings');const settings=await api<RuntimeSettings>('/runtime/settings',{method:'PUT',body:JSON.stringify({node_path:current.node_path||null,agent_paths:current.agent_paths||{},default_agent_id:agentId||null,default_model:model||null})});setDefaultAgentId(settings.default_agent_id);setDefaultModel(settings.default_model);setDefaultAgentAvailable(Boolean(settings.default_agent_id));},[]);
 useEffect(()=>{refresh()},[refresh]);
 return {agents,node,nodeVersion,setNodeVersion,defaultAgentId,defaultModel,defaultAgentAvailable,saveDefaults,error,refresh};
}
