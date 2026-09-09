import { useMemo, useState } from 'react';
import { useAutoScroll } from './hooks/useAutoScroll';
import { useAppController } from './hooks/useAppController';
import { Sidebar } from './components/Sidebar';
import { ChatPanel } from './components/ChatPanel';
import { WorkspacePanel } from './components/WorkspacePanel';
import { AgentCatalog } from './components/AgentCatalog';
import { AgentConfigDialog } from './components/AgentConfigDialog';
import { ChatSelectionDialog } from './components/ChatSelectionDialog';
import { UserPage } from './components/UserPage';
import type { Agent } from './types';
import './styles/account.css';
type Props={sessionId?:string;navigate:(path:string,replace?:boolean)=>void};
export function App({sessionId,navigate}:Props){const c=useAppController(sessionId,navigate);const[configAgent,setConfigAgent]=useState<Agent>();const activityVersion=useMemo(()=>c.session.activity.map(x=>`${x.key}:${x.type}:${x.detail?.length||0}`).join('|'),[c.session.activity]);useAutoScroll(c.chatRef,{contentVersion:`${c.session.messages.length}:${c.session.stream.length}:${activityVersion}:${sessionId||''}`});const openNewChat=()=>{if(c.runtime.defaultAgentAvailable&&c.runtime.defaultAgentId&&c.runtime.defaultModel)c.createChat();else c.setNewChatOpen(true)};return <div className="app" onClick={()=>c.sessionMenu&&c.setSessionMenu(undefined)}>
 {c.startupError&&<div className="startup-error" role="alert"><strong>连接或运行异常</strong><span>{c.startupError}</span><button onClick={()=>{c.runtime.refresh();c.session.refreshSessions();c.workspace.refresh()}}>重试</button></div>}
 <Sidebar agents={c.runtime.agents} sessions={c.session.sessions} active={sessionId} sessionMenu={c.sessionMenu} onNewChat={openNewChat} onSelectSession={id=>navigate(`/sessions/${id}`)} onSessionMenu={id=>c.setSessionMenu(v=>v===id?undefined:v)} onRenameSession={c.renameSession} onTogglePin={c.togglePin} onDeleteSession={c.deleteSession} onOpenCatalog={()=>c.setCatalogOpen(true)} onOpenAccount={()=>c.setAccountOpen(true)} onLogout={c.logout} onHome={()=>navigate('/')}/>
 <ChatPanel current={c.current} currentAgent={c.currentAgent} agents={c.runtime.agents} active={sessionId} models={c.modelState.models} modelLoading={c.modelState.loading} messages={c.session.messages} stream={c.session.stream} activity={c.session.activity} activityOpen={c.session.activityOpen} input={c.input} textareaRef={c.textareaRef} chatRef={c.chatRef} onInputChange={c.setInput} onAgentChange={c.changeAgent} onModelChange={c.changeModel} onSend={c.send} onToggleActivity={()=>c.session.setActivityOpen(v=>!v)} onStop={c.stop} onNewChat={openNewChat} onAutoResize={c.autoResize}/>
 <WorkspacePanel current={c.current} files={c.workspace.files} fileFilter={c.workspace.filter} selectedFile={c.workspace.selectedFile} selectedLine={c.focusLine} diff={c.workspace.diff} tab={c.workspace.tab} onFilterChange={c.workspace.setFilter} onSelectTab={c.workspace.selectTab} onRefresh={c.workspace.refresh} onOpenFile={c.openWorkspaceFile} onCloseFile={()=>{c.workspace.setSelectedFile(undefined);c.setFocusLine(undefined)}}/>
 <ChatSelectionDialog open={c.newChatOpen} agents={c.runtime.agents} onClose={()=>c.setNewChatOpen(false)} onCreate={c.createChat} defaultAgentId={c.runtime.defaultAgentId} defaultModel={c.runtime.defaultModel} onDefaultChange={c.runtime.saveDefaults} onManageAgents={()=>{c.setNewChatOpen(false);c.setCatalogOpen(true)}}/>
 <AgentCatalog open={c.catalogOpen} agents={c.runtime.agents} node={c.runtime.node} nodeVersion={c.runtime.nodeVersion} installingNode={c.installation.installingNode} installingAgent={c.installation.installingAgent} nodeInstallFeedback={c.installation.nodeInstallFeedback} defaultAgentId={c.runtime.defaultAgentId} defaultModel={c.runtime.defaultModel} onDefaultChange={c.runtime.saveDefaults} onClose={()=>c.setCatalogOpen(false)} onNodeVersionChange={c.runtime.setNodeVersion} onInstallNode={c.installation.installNode} onInstallAgent={c.installation.installAgent} onInstallCustomAgent={c.installation.installCustomAgent} onConfigureAgent={setConfigAgent}/>
 <AgentConfigDialog open={Boolean(configAgent)} agent={configAgent} onClose={()=>setConfigAgent(undefined)}/><UserPage open={c.accountOpen} onClose={()=>c.setAccountOpen(false)} onLogout={c.logout}/>
 </div>}
