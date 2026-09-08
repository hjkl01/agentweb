import { useMemo, useRef, useState } from 'react';
import { useAgentRuntime } from './useAgentRuntime';
import { useAgentInstallation } from './useAgentInstallation';
import { useAgentModels } from './useAgentModels';
import { useSession } from './useSession';
import { useWorkspace } from './useWorkspace';
import { api } from '../lib/api';
import { sessionPath } from '../app/router';

export function useAppController(sessionId:string|undefined,navigate:(path:string,replace?:boolean)=>void){
 const runtime=useAgentRuntime(); const [input,setInput]=useState(''); const [catalogOpen,setCatalogOpen]=useState(false); const [accountOpen,setAccountOpen]=useState(false); const [newChatOpen,setNewChatOpen]=useState(false); const [sessionMenu,setSessionMenu]=useState<string>(); const [focusLine,setFocusLine]=useState<number>();
 const chatRef=useRef<HTMLElement>(null); const textareaRef=useRef<HTMLTextAreaElement>(null); const session=useSession(sessionId); const workspace=useWorkspace(sessionId,session.workspaceRevision); const installation=useAgentInstallation(runtime.agents,runtime.nodeVersion,runtime.node,runtime.refresh); const current=useMemo(()=>session.sessions.find(x=>x.id===sessionId),[session.sessions,sessionId]); const currentAgent=useMemo(()=>runtime.agents.find(x=>x.id===current?.agent_id),[runtime.agents,current?.agent_id]); const modelState=useAgentModels(current?.agent_id);
 const startupError=runtime.error||session.error||workspace.error||installation.error;
 const logout=async()=>{try{await api('/auth/logout',{method:'POST'})}finally{window.location.href='/'}};
 const createChat=async(agentId:string,model?:string)=>{try{const created=await session.createSession(agentId,model);navigate(sessionPath(created.id));setNewChatOpen(false);setCatalogOpen(false)}catch(e){alert(e instanceof Error?e.message:String(e))}};
 const send=async()=>{if(!sessionId||!input.trim()||current?.status==='running')return;const message=input.trim();setInput('');setFocusLine(undefined);try{await session.sendChat(sessionId,message)}catch{} };
 const deleteSession=async(id:string)=>{if(!confirm('删除这个会话及其工作区？'))return;try{const next=session.sessions.find(x=>x.id!==id)?.id;await session.deleteSession(id);setSessionMenu(undefined);if(sessionId===id)next?navigate(sessionPath(next)):navigate('/')}catch(e){alert(e instanceof Error?e.message:String(e))}};
 const stop=()=>{if(sessionId)session.interrupt(sessionId).catch(console.error)}; const changeModel=(model?:string)=>{if(sessionId)session.setModel(sessionId,model).catch(console.error)};
 const autoResize=()=>{const t=textareaRef.current;if(!t)return;t.style.height='auto';t.style.height=`${Math.min(t.scrollHeight,180)}px`}; const openWorkspaceFile=(path:string,line?:number)=>{setFocusLine(line);workspace.openFile(path)};
 return {runtime,installation,session,workspace,current,currentAgent,modelState,startupError,input,setInput,catalogOpen,setCatalogOpen,accountOpen,setAccountOpen,newChatOpen,setNewChatOpen,sessionMenu,setSessionMenu,focusLine,setFocusLine,chatRef,textareaRef,logout,createChat,send,deleteSession,stop,changeModel,autoResize,openWorkspaceFile};
}
