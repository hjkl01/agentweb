import { useCallback, useEffect, useState } from 'react';
import { api } from '../lib/api';
import type { Agent, NodeInfo } from '../types';

type RuntimeSettings={node_path?:string;agent_paths:Record<string,string>;default_agent_id?:string;default_model?:string};
export function useAgentRuntime(){
 const [agents,setAgents]=useState<Agent[]>([]);const [node,setNode]=useState<NodeInfo>();const [nodeVersion,setNodeVersion]=useState('');const [defaultAgentId,setDefaultAgentId]=useState<string>();const [defaultModel,setDefaultModel]=useState<string>();const [error,setError]=useState<string>();
 const refresh=useCallback(async()=>{setError(undefined);try{const [catalog,nodeInfo,settings]=await Promise.all([api<Agent[]>('/agent-catalog'),api<NodeInfo>('/node/versions'),api<RuntimeSettings>('/runtime/settings')]);setAgents(catalog);setNode(nodeInfo);setNodeVersion(current=>current||nodeInfo.active||'');setDefaultAgentId(settings.default_agent_id);setDefaultModel(settings.default_model)}catch(error){const message=error instanceof Error?error.message:String(error);console.error('Failed to load Agent runtime:',error);setError(message)}},[]);
 const saveDefaults=useCallback(async(agentId?:string,model?:string)=>{const current=await api<RuntimeSettings>('/runtime/settings');const settings=await api<RuntimeSettings>('/runtime/settings',{method:'PUT',body:JSON.stringify({node_path:current.node_path||null,agent_paths:current.agent_paths||{},default_agent_id:agentId||null,default_model:model||null})});setDefaultAgentId(settings.default_agent_id);setDefaultModel(settings.default_model)},[]);
 useEffect(()=>{refresh()},[refresh]);
 return {agents,node,nodeVersion,setNodeVersion,defaultAgentId,defaultModel,saveDefaults,error,refresh};
}
