import { useEffect, useMemo, useRef, useState } from 'react';
import type { RefObject } from 'react';
import { useAgentRuntime } from './hooks/useAgentRuntime';
import { useAgentInstallation } from './hooks/useAgentInstallation';
import { useAgentModels } from './hooks/useAgentModels';
import { useSession } from './hooks/useSession';
import { useWorkspace } from './hooks/useWorkspace';
import { Sidebar } from './components/Sidebar';
import { ChatPanel } from './components/ChatPanel';
import { WorkspacePanel } from './components/WorkspacePanel';
import { AgentCatalog } from './components/AgentCatalog';
import { NewChatDialog } from './components/NewChatDialog';
import { sessionPath } from './app/router';

type Props = { sessionId?: string; navigate: (path: string, replace?: boolean) => void };
type ComposerRefs = { chat: RefObject<HTMLElement | null>; textarea: RefObject<HTMLTextAreaElement | null> };

export function App({ sessionId, navigate }: Props) {
  const { agents, node, nodeVersion, setNodeVersion, nodeGroups, error: agentError, refresh: refreshAgents } = useAgentRuntime();
  const [input, setInput] = useState('');
  const [catalogOpen, setCatalogOpen] = useState(false);
  const [newChatOpen, setNewChatOpen] = useState(false);
  const [sessionMenu, setSessionMenu] = useState<string>();
  const refs: ComposerRefs = { chat: useRef<HTMLElement>(null), textarea: useRef<HTMLTextAreaElement>(null) };
  const session = useSession(sessionId);
  const workspace = useWorkspace(sessionId, session.workspaceRevision);
  const installation = useAgentInstallation(agents, nodeVersion, node, refreshAgents);
  const current = useMemo(() => session.sessions.find(item => item.id === sessionId), [session.sessions, sessionId]);
  const currentAgent = useMemo(() => agents.find(item => item.id === current?.agent_id), [agents, current?.agent_id]);
  const modelState = useAgentModels(current?.agent_id);
  const startupError = agentError || session.error || workspace.error || installation.error;

  useEffect(() => {
    if (!sessionId && session.sessions[0]) navigate(sessionPath(session.sessions[0].id), true);
  }, [navigate, sessionId, session.sessions]);

  useEffect(() => {
    const chat = refs.chat.current;
    if (!chat) return;
    requestAnimationFrame(() => chat.scrollTo({ top: chat.scrollHeight, behavior: 'smooth' }));
  }, [session.messages, session.stream, session.activity]);

  const createChat = async (agentId: string, model?: string) => {
    try {
      const created = await session.createSession(agentId, model);
      navigate(sessionPath(created.id));
      setNewChatOpen(false); setCatalogOpen(false);
    } catch (error) { alert(error instanceof Error ? error.message : String(error)); }
  };

  const send = async () => {
    if (!sessionId || !input.trim() || current?.status === 'running') return;
    const message = input.trim(); setInput('');
    session.setMessages(items => [...items, { id: crypto.randomUUID(), role: 'user', content: message }]);
    try { await session.sendMessage(sessionId, message); }
    catch (error) { session.setMessages(items => [...items, { id: crypto.randomUUID(), role: 'error', content: error instanceof Error ? error.message : String(error) }]); }
  };

  const deleteSession = async (id: string) => {
    if (!confirm('删除这个会话？')) return;
    try {
      const next = session.sessions.find(item => item.id !== id)?.id;
      await session.deleteSession(id); setSessionMenu(undefined);
      if (sessionId === id) next ? navigate(sessionPath(next)) : navigate('/');
    } catch (error) { alert(error instanceof Error ? error.message : String(error)); }
  };

  const stop = () => { if (sessionId) session.interrupt(sessionId).catch(console.error); };
  const changeModel = (model?: string) => { if (sessionId) session.setModel(sessionId, model).catch(console.error); };
  const autoResize = () => {
    const textarea = refs.textarea.current; if (!textarea) return;
    textarea.style.height = 'auto'; textarea.style.height = `${Math.min(textarea.scrollHeight, 180)}px`;
  };

  return (
    <div className="app" onClick={() => sessionMenu && setSessionMenu(undefined)}>
      {startupError && <div className="startup-error" role="alert"><strong>连接或运行异常</strong><span>{startupError}</span><button onClick={() => { refreshAgents(); session.refreshSessions(); workspace.refresh(); }}>重试</button></div>}
      <Sidebar agents={agents} sessions={session.sessions} active={sessionId} sessionMenu={sessionMenu} onNewChat={() => setNewChatOpen(true)} onSelectSession={id => navigate(sessionPath(id))} onSessionMenu={id => setSessionMenu(value => value === id ? undefined : id)} onDeleteSession={deleteSession} onOpenCatalog={() => setCatalogOpen(true)} />
      <ChatPanel current={current} currentAgent={currentAgent} active={sessionId} models={modelState.models} modelLoading={modelState.loading} messages={session.messages} stream={session.stream} activity={session.activity} activityOpen={session.activityOpen} input={input} textareaRef={refs.textarea} chatRef={refs.chat} onInputChange={setInput} onModelChange={changeModel} onSend={send} onToggleActivity={() => session.setActivityOpen(value => !value)} onStop={stop} onNewChat={() => setNewChatOpen(true)} onAutoResize={autoResize} />
      <WorkspacePanel current={current} files={workspace.files} fileFilter={workspace.filter} selectedFile={workspace.selectedFile} diff={workspace.diff} tab={workspace.tab} onFilterChange={workspace.setFilter} onSelectTab={workspace.selectTab} onRefresh={workspace.refresh} onOpenFile={workspace.openFile} onCloseFile={() => workspace.setSelectedFile(undefined)} />
      <NewChatDialog open={newChatOpen} agents={agents} onClose={() => setNewChatOpen(false)} onSelectAgent={createChat} onManageAgents={() => { setNewChatOpen(false); setCatalogOpen(true); }} />
      <AgentCatalog open={catalogOpen} agents={agents} node={node} nodeVersion={nodeVersion} installingNode={installation.installingNode} installingAgent={installation.installingAgent} nodeGroups={nodeGroups} onClose={() => setCatalogOpen(false)} onNodeVersionChange={setNodeVersion} onInstallNode={installation.installNode} onInstallAgent={installation.installAgent} />
    </div>
  );
}
