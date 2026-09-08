import { useMemo, useRef, useState } from 'react';
import { useAgentRuntime } from './hooks/useAgentRuntime';
import { useAgentInstallation } from './hooks/useAgentInstallation';
import { useAgentModels } from './hooks/useAgentModels';
import { useSession } from './hooks/useSession';
import { useWorkspace } from './hooks/useWorkspace';
import { useAutoScroll } from './hooks/useAutoScroll';
import { Sidebar } from './components/Sidebar';
import { ChatPanel } from './components/ChatPanel';
import { WorkspacePanel } from './components/WorkspacePanel';
import { AgentCatalog } from './components/AgentCatalog';
import { NewChatDialog } from './components/NewChatDialog';
import { sessionPath } from './app/router';
import { api } from './lib/api';

type Props = { sessionId?: string; navigate: (path: string, replace?: boolean) => void };

export function App({ sessionId, navigate }: Props) {
  const { agents, node, nodeVersion, setNodeVersion, nodeGroups, error: agentError, refresh: refreshAgents } = useAgentRuntime();
  const [input, setInput] = useState('');
  const [catalogOpen, setCatalogOpen] = useState(false);
  const [newChatOpen, setNewChatOpen] = useState(false);
  const [sessionMenu, setSessionMenu] = useState<string>();
  const [focusLine, setFocusLine] = useState<number>();
  const chatRef = useRef<HTMLElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const session = useSession(sessionId);
  const workspace = useWorkspace(sessionId, session.workspaceRevision);
  const installation = useAgentInstallation(agents, nodeVersion, node, refreshAgents);
  const current = useMemo(() => session.sessions.find(item => item.id === sessionId), [session.sessions, sessionId]);
  const currentAgent = useMemo(() => agents.find(item => item.id === current?.agent_id), [agents, current?.agent_id]);
  const modelState = useAgentModels(current?.agent_id);
  const startupError = agentError || session.error || workspace.error || installation.error;
  const activityVersion = session.activity.map(item => `${item.key}:${item.type}:${item.detail?.length || 0}`).join('|');
  useAutoScroll(chatRef, { contentVersion: `${session.messages.length}:${session.stream.length}:${activityVersion}:${sessionId || ''}` });

  const logout = async () => {
    try { await api('/auth/logout', { method: 'POST' }); } finally { window.location.href = '/'; }
  };
  const createChat = async (agentId: string, model?: string) => {
    try { const created = await session.createSession(agentId, model); navigate(sessionPath(created.id)); setNewChatOpen(false); setCatalogOpen(false); } catch (error) { alert(error instanceof Error ? error.message : String(error)); }
  };
  const send = async () => {
    if (!sessionId || !input.trim() || current?.status === 'running') return;
    const message = input.trim(); setInput(''); setFocusLine(undefined); session.setActivity([]); session.setActivityOpen(false);
    session.setMessages(items => [...items, { id: crypto.randomUUID(), role: 'user', content: message }]);
    try { await session.sendMessage(sessionId, message); } catch (error) { session.setMessages(items => [...items, { id: crypto.randomUUID(), role: 'error', content: error instanceof Error ? error.message : String(error) }]); }
  };
  const deleteSession = async (id: string) => {
    if (!confirm('删除这个会话？')) return;
    try { const next = session.sessions.find(item => item.id !== id)?.id; await session.deleteSession(id); setSessionMenu(undefined); if (sessionId === id) next ? navigate(sessionPath(next)) : navigate('/'); } catch (error) { alert(error instanceof Error ? error.message : String(error)); }
  };
  const stop = () => { if (sessionId) session.interrupt(sessionId).catch(console.error); };
  const changeModel = (model?: string) => { if (sessionId) session.setModel(sessionId, model).catch(console.error); };
  const autoResize = () => { const textarea = textareaRef.current; if (!textarea) return; textarea.style.height = 'auto'; textarea.style.height = `${Math.min(textarea.scrollHeight, 180)}px`; };
  const openWorkspaceFile = (path: string, line?: number) => { setFocusLine(line); workspace.openFile(path); };

  return <div className="app" onClick={() => sessionMenu && setSessionMenu(undefined)}>
    {startupError && <div className="startup-error" role="alert"><strong>连接或运行异常</strong><span>{startupError}</span><button onClick={() => { refreshAgents(); session.refreshSessions(); workspace.refresh(); }}>重试</button></div>}
    <Sidebar agents={agents} sessions={session.sessions} active={sessionId} sessionMenu={sessionMenu} onNewChat={() => setNewChatOpen(true)} onSelectSession={id => navigate(sessionPath(id))} onSessionMenu={id => setSessionMenu(value => value === id ? undefined : id)} onDeleteSession={deleteSession} onOpenCatalog={() => setCatalogOpen(true)} onLogout={logout} />
    <ChatPanel current={current} currentAgent={currentAgent} active={sessionId} models={modelState.models} modelLoading={modelState.loading} messages={session.messages} stream={session.stream} activity={session.activity} activityOpen={session.activityOpen} input={input} textareaRef={textareaRef} chatRef={chatRef} onInputChange={setInput} onModelChange={changeModel} onSend={send} onToggleActivity={() => session.setActivityOpen(value => !value)} onStop={stop} onNewChat={() => setNewChatOpen(true)} onAutoResize={autoResize} />
    <WorkspacePanel current={current} files={workspace.files} fileFilter={workspace.filter} selectedFile={workspace.selectedFile} selectedLine={focusLine} diff={workspace.diff} tab={workspace.tab} onFilterChange={workspace.setFilter} onSelectTab={workspace.selectTab} onRefresh={workspace.refresh} onOpenFile={openWorkspaceFile} onCloseFile={() => { workspace.setSelectedFile(undefined); setFocusLine(undefined); }} />
    <NewChatDialog open={newChatOpen} agents={agents} onClose={() => setNewChatOpen(false)} onSelectAgent={createChat} onManageAgents={() => { setNewChatOpen(false); setCatalogOpen(true); }} />
    <AgentCatalog open={catalogOpen} agents={agents} node={node} nodeVersion={nodeVersion} installingNode={installation.installingNode} installingAgent={installation.installingAgent} nodeGroups={nodeGroups} onClose={() => setCatalogOpen(false)} onNodeVersionChange={setNodeVersion} onInstallNode={installation.installNode} onInstallAgent={installation.installAgent} />
  </div>;
}