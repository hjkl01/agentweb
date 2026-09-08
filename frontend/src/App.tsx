import { useMemo } from 'react';
import { useAutoScroll } from './hooks/useAutoScroll';
import { useAppController } from './hooks/useAppController';
import { Sidebar } from './components/Sidebar';
import { ChatPanel } from './components/ChatPanel';
import { WorkspacePanel } from './components/WorkspacePanel';
import { AgentCatalog } from './components/AgentCatalog';
import { NewChatDialog } from './components/NewChatDialog';
import { UserPage } from './components/UserPage';
import './styles/account.css';

type Props={sessionId?:string;navigate:(path:string,replace?:boolean)=>void};
export function App({sessionId,navigate}:Props){
 const c=useAppController(sessionId,navigate); const activityVersion=useMemo(()=>c.session.activity.map(x=>`${x.key}:${x.type}:${x.detail?.length||0}`).join('|'),[c.session.activity]);
 useAutoScroll(c.chatRef,{contentVersion:`${c.session.messages.length}:${c.session.stream.length}:${activityVersion}:${sessionId||''}`});
 return <div className="app" onClick={()=>c.sessionMenu&&c.setSessionMenu(undefined)}>
  {c.startupError&&<div className="startup-error" role="alert"><strong>连接或运行异常</strong><span>{c.startupError}</span><button onClick={()=>{c.runtime.refresh();c.session.refreshSessions();c.workspace.refresh()}}>重试</button></div>}
  <Sidebar agents={c.runtime.agents} sessions={c.session.sessions} active={sessionId} sessionMenu={c.sessionMenu} onNewChat={()=>c.setNewChatOpen(true)} onSelectSession={id=>navigate(`/sessions/${id}`)} onSessionMenu={id=>c.setSessionMenu(v=>v===id?undefined:v)} onDeleteSession={c.deleteSession} onOpenCatalog={()=>c.setCatalogOpen(true)} onOpenAccount={()=>c.setAccountOpen(true)} onLogout={c.logout}/>
  <ChatPanel current={c.current} currentAgent={c.currentAgent} active={sessionId} models={c.modelState.models} modelLoading={c.modelState.loading} messages={c.session.messages} stream={c.session.stream} activity={c.session.activity} activityOpen={c.session.activityOpen} input={c.input} textareaRef={c.textareaRef} chatRef={c.chatRef} onInputChange={c.setInput} onModelChange={c.changeModel} onSend={c.send} onToggleActivity={()=>c.session.setActivityOpen(v=>!v)} onStop={c.stop} onNewChat={()=>c.setNewChatOpen(true)} onAutoResize={c.autoResize}/>
  <WorkspacePanel current={c.current} files={c.workspace.files} fileFilter={c.workspace.filter} selectedFile={c.workspace.selectedFile} selectedLine={c.focusLine} diff={c.workspace.diff} tab={c.workspace.tab} onFilterChange={c.workspace.setFilter} onSelectTab={c.workspace.selectTab} onRefresh={c.workspace.refresh} onOpenFile={c.openWorkspaceFile} onCloseFile={()=>{c.workspace.setSelectedFile(undefined);c.setFocusLine(undefined)}}/>
  <NewChatDialog open={c.newChatOpen} agents={c.runtime.agents} onClose={()=>c.setNewChatOpen(false)} onSelectAgent={c.createChat} onManageAgents={()=>{c.setNewChatOpen(false);c.setCatalogOpen(true)}}/>
  <AgentCatalog open={c.catalogOpen} agents={c.runtime.agents} node={c.runtime.node} nodeVersion={c.runtime.nodeVersion} installingNode={c.installation.installingNode} installingAgent={c.installation.installingAgent} nodeGroups={c.runtime.nodeGroups} onClose={()=>c.setCatalogOpen(false)} onNodeVersionChange={c.runtime.setNodeVersion} onInstallNode={c.installation.installNode} onInstallAgent={c.installation.installAgent}/>
  <UserPage open={c.accountOpen} onClose={()=>c.setAccountOpen(false)} onLogout={c.logout}/>
 </div>;
}
