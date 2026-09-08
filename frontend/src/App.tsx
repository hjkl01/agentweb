import { useEffect, useMemo, useRef, useState } from 'react';
import { api, fileUrl } from './lib/api';
import { useAgentRuntime } from './hooks/useAgentRuntime';
import { useSession } from './hooks/useSession';
import { Sidebar } from './components/Sidebar';
import { ChatPanel } from './components/ChatPanel';
import { WorkspacePanel } from './components/WorkspacePanel';
import { AgentCatalog } from './components/AgentCatalog';
import { NewChatDialog } from './components/NewChatDialog';
import type { WorkspaceFile } from './types';

export function App() {
  const { agents, node, nodeVersion, setNodeVersion, nodeGroups, error: agentError, refresh: refreshAgents } = useAgentRuntime();
  const [active, setActive] = useState<string>();
  const [input, setInput] = useState('');
  const [catalogOpen, setCatalogOpen] = useState(false);
  const [newChatOpen, setNewChatOpen] = useState(false);
  const [sessionMenu, setSessionMenu] = useState<string>();
  const [workspaceTab, setWorkspaceTab] = useState<'files' | 'diff'>('files');
  const [fileFilter, setFileFilter] = useState('');
  const [selectedFile, setSelectedFile] = useState<WorkspaceFile>();
  const [diff, setDiff] = useState<any>();
  const [installingNode, setInstallingNode] = useState(false);
  const [installingAgent, setInstallingAgent] = useState<string>();
  const chatRef = useRef<HTMLElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const session = useSession(active);
  const current = useMemo(() => session.sessions.find(item => item.id === active), [session.sessions, active]);
  const currentAgent = useMemo(() => agents.find(item => item.id === current?.agent_id), [agents, current]);
  const startupError = agentError || session.error;

  useEffect(() => {
    if (!active && session.sessions[0]) setActive(session.sessions[0].id);
  }, [active, session.sessions]);

  useEffect(() => {
    if (chatRef.current) chatRef.current.scrollTop = chatRef.current.scrollHeight;
  }, [session.messages, session.stream, session.activity]);

  const createChat = async (agentId: string) => {
    try {
      const created = await session.createSession(agentId);
      setActive(created.id);
      setNewChatOpen(false);
      setCatalogOpen(false);
    } catch (error: any) {
      alert(error.message);
    }
  };

  const send = async () => {
    if (!active || !input.trim() || current?.status === 'running') return;
    const message = input.trim();
    setInput('');
    session.setMessages(items => [...items, { id: crypto.randomUUID(), role: 'user', content: message }]);
    try {
      await session.sendMessage(active, message);
    } catch (error: any) {
      session.setMessages(items => [...items, { id: crypto.randomUUID(), role: 'error', content: error.message }]);
    }
  };

  const deleteSession = async (id: string) => {
    if (!confirm('删除这个会话？')) return;
    try {
      const nextActive = session.sessions.find(item => item.id !== id)?.id;
      await session.deleteSession(id);
      setSessionMenu(undefined);
      if (active === id) setActive(nextActive);
    } catch (error: any) {
      alert(error.message);
    }
  };

  const installNode = async () => {
    if (!nodeVersion) return;
    setInstallingNode(true);
    try {
      await api('/node/install', { method: 'POST', body: JSON.stringify({ version: nodeVersion }) });
      await refreshAgents();
    } catch (error: any) {
      alert(error.message);
    } finally {
      setInstallingNode(false);
    }
  };

  const installAgent = async (id: string) => {
    const agent = agents.find(item => item.id === id);
    if (agent?.requirements.includes('Node.js') && !node?.installed.includes(nodeVersion)) {
      alert('请先选择并安装一个 Node.js 版本');
      return;
    }
    setInstallingAgent(id);
    try {
      await api(`/agents/${id}/install`, { method: 'POST' });
      await refreshAgents();
    } catch (error: any) {
      alert(error.message);
    } finally {
      setInstallingAgent(undefined);
    }
  };

  const refreshWorkspace = async () => {
    if (!active) return;
    session.setFiles(await api(`/sessions/${active}/files`));
    if (workspaceTab === 'diff') setDiff(await api(`/sessions/${active}/diff`));
  };

  const openFile = async (path: string) => {
    if (!active) return;
    try {
      setSelectedFile(await api<WorkspaceFile>(fileUrl(active, path)));
    } catch (error: any) {
      setSelectedFile({ path, content: error.message });
    }
  };

  const selectWorkspaceTab = async (tab: 'files' | 'diff') => {
    setWorkspaceTab(tab);
    if (tab === 'diff' && active) {
      try { setDiff(await api(`/sessions/${active}/diff`)); }
      catch (error: any) { setDiff({ diff: error.message }); }
    }
  };

  const stop = () => {
    if (active) api(`/sessions/${active}/interrupt`, { method: 'POST' }).catch(() => {});
  };

  const autoResize = () => {
    const textarea = textareaRef.current;
    if (!textarea) return;
    textarea.style.height = 'auto';
    textarea.style.height = `${Math.min(textarea.scrollHeight, 180)}px`;
  };

  return (
    <div className="app" onClick={() => sessionMenu && setSessionMenu(undefined)}>
      {startupError && (
        <div className="startup-error" role="alert">
          <strong>后端连接异常</strong>
          <span>{startupError}</span>
          <button onClick={() => { refreshAgents(); session.refreshSessions(); }}>重试</button>
        </div>
      )}

      <Sidebar
        agents={agents}
        sessions={session.sessions}
        active={active}
        sessionMenu={sessionMenu}
        onNewChat={() => setNewChatOpen(true)}
        onSelectSession={setActive}
        onSessionMenu={id => setSessionMenu(value => value === id ? undefined : id)}
        onDeleteSession={deleteSession}
        onOpenCatalog={() => setCatalogOpen(true)}
      />

      <ChatPanel
        current={current}
        currentAgent={currentAgent}
        active={active}
        messages={session.messages}
        stream={session.stream}
        activity={session.activity}
        activityOpen={session.activityOpen}
        input={input}
        textareaRef={textareaRef}
        chatRef={chatRef}
        onInputChange={setInput}
        onSend={send}
        onToggleActivity={() => session.setActivityOpen(value => !value)}
        onStop={stop}
        onNewChat={() => setNewChatOpen(true)}
        onAutoResize={autoResize}
      />

      <WorkspacePanel
        current={current}
        files={session.files}
        fileFilter={fileFilter}
        selectedFile={selectedFile}
        diff={diff}
        tab={workspaceTab}
        onFilterChange={setFileFilter}
        onSelectTab={selectWorkspaceTab}
        onRefresh={refreshWorkspace}
        onOpenFile={openFile}
        onCloseFile={() => setSelectedFile(undefined)}
      />

      <NewChatDialog
        open={newChatOpen}
        agents={agents}
        onClose={() => setNewChatOpen(false)}
        onSelectAgent={createChat}
        onManageAgents={() => { setNewChatOpen(false); setCatalogOpen(true); }}
      />

      <AgentCatalog
        open={catalogOpen}
        agents={agents}
        node={node}
        nodeVersion={nodeVersion}
        installingNode={installingNode}
        installingAgent={installingAgent}
        nodeGroups={nodeGroups}
        onClose={() => setCatalogOpen(false)}
        onNodeVersionChange={setNodeVersion}
        onInstallNode={installNode}
        onInstallAgent={installAgent}
      />
    </div>
  );
}
