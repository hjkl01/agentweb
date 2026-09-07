import React, { createRoot, useEffect, useMemo, useRef, useState } from 'react';
import {
  Activity, Bot, ChevronDown, ChevronRight, File, Folder, FolderOpen,
  GitCompare, MessageCircle, MoreHorizontal, Plus, RefreshCw, Search,
  Send, Settings, Square, Trash2, X, Download, CheckCircle2, Circle,
} from 'lucide-react';
import './style.css';

type Agent = { id: string; name: string; description?: string; installed: boolean; requirements: string[] };
type Session = { id: string; agent_id: string; title: string; workspace: string; status: string; native_session_id?: string };
type ChatMessage = { id: string; role: string; content: string; created_at?: string };
type ActivityItem = { id: string; type: string; label: string; detail?: string };
type NodeInfo = { available: string[]; installed: string[]; active?: string };
type FileItem = { path: string; kind: string };

const api = (path: string, options?: RequestInit) =>
  fetch('/api' + path, { headers: { 'Content-Type': 'application/json' }, ...options }).then(async response => {
    const data = await response.json().catch(() => ({}));
    if (!response.ok) throw new Error(data.error || response.statusText);
    return data;
  });

function App() {
  const [agents, setAgents] = useState<Agent[]>([]);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [active, setActive] = useState<string>();
  const [input, setInput] = useState('');
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [stream, setStream] = useState('');
  const [activity, setActivity] = useState<ActivityItem[]>([]);
  const [activityOpen, setActivityOpen] = useState(false);
  const [catalog, setCatalog] = useState(false);
  const [files, setFiles] = useState<FileItem[]>([]);
  const [selectedFile, setSelectedFile] = useState<any>();
  const [diff, setDiff] = useState<any>();
  const [workspaceTab, setWorkspaceTab] = useState<'files' | 'diff'>('files');
  const [node, setNode] = useState<NodeInfo>();
  const [nodeVersion, setNodeVersion] = useState('');
  const [installingNode, setInstallingNode] = useState(false);
  const [installingAgent, setInstallingAgent] = useState<string>();
  const [newChatOpen, setNewChatOpen] = useState(false);
  const [sessionMenu, setSessionMenu] = useState<string>();
  const [fileFilter, setFileFilter] = useState('');
  const chatRef = useRef<HTMLElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const refreshAgents = async () => {
    const [catalogData, nodeData] = await Promise.all([api('/agent-catalog'), api('/node/versions')]);
    setAgents(catalogData);
    setNode(nodeData);
    setNodeVersion(current => current || nodeData.active || '');
  };

  useEffect(() => {
    Promise.all([refreshAgents(), api('/sessions')]).then(([, sessionData]) => {
      setSessions(sessionData);
      if (sessionData[0]) setActive(sessionData[0].id);
    }).catch(() => {});
  }, []);

  useEffect(() => {
    if (!active) return;
    setStream('');
    setActivity([]);
    setActivityOpen(false);
    Promise.all([api(`/sessions/${active}/messages`), api(`/sessions/${active}/files`)]).then(([m, f]) => {
      setMessages(m);
      setFiles(f);
    }).catch(() => {});

    const ws = new WebSocket(`${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}/api/sessions/${active}/events`);
    ws.onmessage = event => {
      const ev = JSON.parse(event.data), data = ev.data || {};
      const add = (label: string, detail?: string) => {
        setActivity(items => [...items, { id: crypto.randomUUID(), type: ev.type, label, detail }].slice(-40));
        setActivityOpen(true);
      };
      if (ev.type === 'message.started') add('Agent started');
      if (ev.type === 'message.delta') setStream(value => value + (data.text || ''));
      if (ev.type === 'message.completed') {
        setStream(value => {
          if (value) setMessages(items => [...items, { id: crypto.randomUUID(), role: 'assistant', content: value }]);
          return '';
        });
      }
      if (ev.type === 'thinking.started') add('Thinking');
      if (ev.type === 'thinking.delta') add('Thinking', data.text);
      if (ev.type === 'thinking.completed') add('Thinking completed');
      if (ev.type === 'tool.started') add(`Tool: ${data.tool}`);
      if (ev.type === 'tool.output') add(`Tool output: ${data.tool}`, data.output);
      if (ev.type === 'tool.completed') add(`Tool completed: ${data.tool}`);
      if (ev.type === 'command.started') add(`Command: ${data.command}`);
      if (ev.type === 'command.output') add('Command output', data.output);
      if (ev.type === 'command.completed') add('Command completed');
      if (ev.type === 'file.created') add('File created', data.path);
      if (ev.type === 'file.modified') add('File modified', data.path);
      if (ev.type === 'file.deleted') add('File deleted', data.path);
      if (ev.type === 'agent.error' || ev.type === 'error') add('Agent error', data.message);
      if (ev.type === 'session.completed' || ev.type === 'session.error') {
        api(`/sessions/${active}/messages`).then(setMessages).catch(() => {});
        api(`/sessions/${active}/files`).then(setFiles).catch(() => {});
        api('/sessions').then(setSessions).catch(() => {});
      }
    };
    return () => ws.close();
  }, [active]);

  useEffect(() => {
    const element = chatRef.current;
    if (element) element.scrollTop = element.scrollHeight;
  }, [messages, stream, activity]);

  const current = useMemo(() => sessions.find(session => session.id === active), [sessions, active]);
  const currentAgent = useMemo(() => agents.find(agent => agent.id === current?.agent_id), [agents, current]);
  const installedAgents = agents.filter(agent => agent.installed);
  const filteredFiles = files.filter(file => file.path.toLowerCase().includes(fileFilter.toLowerCase()));
  const nodeGroups = Object.entries((node?.available || []).reduce((groups, version) => {
    const major = version.split('.')[0];
    (groups[major] ??= []).push(version);
    return groups;
  }, {} as Record<string, string[]>)).sort(([a], [b]) => Number(b) - Number(a));

  const createChat = async (agentId: string) => {
    try {
      const session = await api('/sessions', {
        method: 'POST',
        body: JSON.stringify({ agent_id: agentId, workspace: '/workspaces', title: 'New Chat' }),
      });
      setSessions(items => [session, ...items]);
      setActive(session.id);
      setNewChatOpen(false);
    } catch (error: any) { alert(error.message); }
  };

  const send = async () => {
    if (!active || !input.trim() || current?.status === 'running') return;
    const message = input.trim();
    setInput('');
    setMessages(items => [...items, { id: crypto.randomUUID(), role: 'user', content: message }]);
    try {
      await api(`/sessions/${active}/messages`, { method: 'POST', body: JSON.stringify({ message }) });
      setSessions(items => items.map(item => item.id === active ? { ...item, status: 'running' } : item));
    } catch (error: any) {
      setMessages(items => [...items, { id: crypto.randomUUID(), role: 'error', content: error.message }]);
    }
  };

  const deleteSession = async (id: string) => {
    if (!confirm('删除这个会话？')) return;
    try {
      await api(`/sessions/${id}`, { method: 'DELETE' });
      const remaining = sessions.filter(session => session.id !== id);
      setSessions(remaining);
      setSessionMenu(undefined);
      if (active === id) setActive(remaining[0]?.id);
    } catch (error: any) { alert(error.message); }
  };

  const installNode = async () => {
    if (!nodeVersion) return;
    setInstallingNode(true);
    try { await api('/node/install', { method: 'POST', body: JSON.stringify({ version: nodeVersion }) }); await refreshAgents(); }
    catch (error: any) { alert(error.message); }
    finally { setInstallingNode(false); }
  };

  const installAgent = async (id: string) => {
    const agent = agents.find(item => item.id === id);
    if (agent?.requirements.includes('Node.js') && !node?.installed.includes(nodeVersion)) {
      alert('请先选择并安装一个 Node.js 版本');
      return;
    }
    setInstallingAgent(id);
    try { await api(`/agents/${id}/install`, { method: 'POST' }); setTimeout(refreshAgents, 1200); }
    catch (error: any) { alert(error.message); }
    finally { setTimeout(() => setInstallingAgent(undefined), 1200); }
  };

  const refreshWorkspace = async () => {
    if (!active) return;
    const data = await api(`/sessions/${active}/files`);
    setFiles(data);
    if (workspaceTab === 'diff') setDiff(await api(`/sessions/${active}/diff`));
  };

  const openFile = async (path: string) => {
    if (!active) return;
    try { setSelectedFile(await api(`/sessions/${active}/file/${path.split('/').map(encodeURIComponent).join('/')}`)); }
    catch (error: any) { setSelectedFile({ path, content: error.message }); }
  };

  const autoResize = () => {
    const textarea = textareaRef.current;
    if (!textarea) return;
    textarea.style.height = 'auto';
    textarea.style.height = `${Math.min(textarea.scrollHeight, 180)}px`;
  };

  return <div className="app" onClick={() => sessionMenu && setSessionMenu(undefined)}>
    <aside className="sidebar">
      <div className="brand"><div className="brand-mark"><Bot size={18}/></div><div><strong>Agent Web</strong><span>Agent workspace</span></div></div>
      <button className="new-chat" onClick={e => { e.stopPropagation(); setNewChatOpen(true); }}><Plus size={16}/> New chat</button>
      <div className="section-label">Chats <span>{sessions.length}</span></div>
      <div className="session-list">
        {sessions.map(session => <div key={session.id} className={`session ${active === session.id ? 'active' : ''}`} onClick={() => setActive(session.id)}>
          <MessageCircle size={15}/><div className="session-main"><span>{session.title}</span><small>{agents.find(a => a.id === session.agent_id)?.name || session.agent_id}</small></div>
          <button className="icon-button session-menu" onClick={e => { e.stopPropagation(); setSessionMenu(sessionMenu === session.id ? undefined : session.id); }}><MoreHorizontal size={15}/></button>
          {sessionMenu === session.id && <div className="menu" onClick={e => e.stopPropagation()}><button onClick={() => deleteSession(session.id)}><Trash2 size={14}/> Delete</button></div>}
        </div>)}
        {!sessions.length && <div className="empty-sidebar">No chats yet</div>}
      </div>
      <div className="sidebar-bottom">
        <button className="settings-button" onClick={() => setCatalog(true)}><Settings size={16}/> Agents & runtimes</button>
      </div>
    </aside>

    <main className="main-panel">
      <header className="topbar">
        <div className="chat-title"><strong>{current?.title || 'Agent Web'}</strong>{currentAgent && <span><span className="status-dot"/> {currentAgent.name}</span>}</div>
        {active && <div className="top-actions"><span className={`run-state ${current?.status === 'running' ? 'running' : ''}`}>{current?.status || 'ready'}</span>{current?.status === 'running' && <button className="stop" onClick={() => api(`/sessions/${active}/interrupt`, { method: 'POST' })}><Square size={13}/> Stop</button>}</div>}
      </header>

      <section className="chat" ref={chatRef}>
        {!active && <div className="welcome"><div className="welcome-icon"><Bot size={30}/></div><h1>Build with your Agent</h1><p>选择一个 Agent 开始新的会话。Agent Web 负责会话、文件和实时交互，Agent 负责实际执行任务。</p><button className="new-chat" onClick={() => setNewChatOpen(true)}><Plus size={16}/> New chat</button></div>}
        {active && !messages.length && !stream && <div className="welcome compact"><div className="welcome-icon"><MessageCircle size={24}/></div><h2>How can I help?</h2><p>向 {currentAgent?.name || current?.agent_id} 描述你想完成的任务。</p></div>}
        {messages.map(message => <div key={message.id} className={`message-row ${message.role}`}><div className="avatar">{message.role === 'user' ? 'U' : message.role === 'error' ? '!' : <Bot size={15}/>}</div><div className="message-content"><div className="message-role">{message.role === 'user' ? 'You' : message.role === 'error' ? 'Error' : currentAgent?.name || 'Agent'}</div><pre>{message.content}</pre></div></div>)}
        {stream && <div className="message-row assistant"><div className="avatar"><Bot size={15}/></div><div className="message-content"><div className="message-role">{currentAgent?.name || 'Agent'} <span className="streaming">streaming</span></div><pre>{stream}<span className="cursor"/></pre></div></div>}
        {activity.length > 0 && <div className="activity-card"><button className="activity-header" onClick={() => setActivityOpen(value => !value)}>{activityOpen ? <ChevronDown size={15}/> : <ChevronRight size={15}/>}<Activity size={14}/><span>Agent activity</span><small>{activity.length} events</small></button>{activityOpen && <div className="activity-list">{activity.map(item => <div className="activity-item" key={item.id}><Circle size={7}/><div><span>{item.label}</span>{item.detail && <pre>{item.detail}</pre>}</div></div>)}</div>}</div>}
      </section>

      <div className="composer-wrap"><div className="composer"><textarea ref={textareaRef} value={input} onChange={e => { setInput(e.target.value); autoResize(); }} onKeyDown={e => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send(); } }} placeholder={active ? 'Message your agent…' : 'Create a chat to start'} disabled={!active || current?.status === 'running'} rows={1}/><button className="send-button" onClick={send} disabled={!active || !input.trim() || current?.status === 'running'}>{current?.status === 'running' ? <Square size={16}/> : <Send size={16}/>}</button></div><div className="composer-hint">Enter 发送 · Shift + Enter 换行</div></div>
    </main>

    <aside className="workspace">
      <div className="workspace-head"><div><strong>Workspace</strong>{current && <small>{current.workspace}</small>}</div><button className="icon-button" onClick={refreshWorkspace} title="Refresh"><RefreshCw size={15}/></button></div>
      <div className="tabs"><button className={workspaceTab === 'files' ? 'active' : ''} onClick={() => setWorkspaceTab('files')}><Folder size={14}/> Files</button><button className={workspaceTab === 'diff' ? 'active' : ''} onClick={async () => { setWorkspaceTab('diff'); if (active) setDiff(await api(`/sessions/${active}/diff`)); }}><GitCompare size={14}/> Diff</button></div>
      {workspaceTab === 'files' && <><div className="file-search"><Search size={14}/><input value={fileFilter} onChange={e => setFileFilter(e.target.value)} placeholder="Filter files"/></div><div className="filetree">{filteredFiles.map(file => <button key={file.path} className="file" onClick={() => file.kind === 'file' && openFile(file.path)}>{file.kind === 'directory' ? <Folder size={14}/> : <File size={14}/>}<span>{file.path}</span></button>)}{!filteredFiles.length && <div className="workspace-empty">No files</div>}</div></>}
      {workspaceTab === 'diff' && <pre className="diff">{diff?.status}{'\n'}{diff?.diff || 'No git diff'}</pre>}
      {selectedFile && <div className="preview"><div className="preview-head"><div><strong>{selectedFile.path}</strong><small>File preview</small></div><button className="icon-button" onClick={() => setSelectedFile(undefined)}><X size={15}/></button></div><pre>{selectedFile.content}</pre></div>}
    </aside>

    {newChatOpen && <div className="modal-backdrop" onClick={() => setNewChatOpen(false)}><div className="modal" onClick={e => e.stopPropagation()}><div className="modal-head"><div><h3>New chat</h3><p>选择要运行的 Agent。</p></div><button className="icon-button" onClick={() => setNewChatOpen(false)}><X size={16}/></button></div>{installedAgents.length ? <div className="agent-choice-list">{installedAgents.map(agent => <button className="agent-choice" key={agent.id} onClick={() => createChat(agent.id)}><div className="agent-choice-icon"><Bot size={17}/></div><div><strong>{agent.name}</strong><span>{agent.description}</span></div><ChevronRight size={16}/></button>)}</div> : <div className="modal-empty"><Bot size={24}/><strong>No Agent installed</strong><p>先到 Agents & runtimes 安装一个 Agent。</p><button className="new-chat" onClick={() => { setNewChatOpen(false); setCatalog(true); }}>Manage agents</button></div>}</div></div>}

    {catalog && <div className="modal-backdrop" onClick={() => setCatalog(false)}><div className="agent-panel" onClick={e => e.stopPropagation()}><div className="modal-head"><div><h3>Agents & runtimes</h3><p>Agent 不预装，Node.js 也不预装。</p></div><button className="icon-button" onClick={() => setCatalog(false)}><X size={16}/></button></div>
      <div className="runtime-card"><div className="card-title"><div><strong>Node.js runtime</strong><span>每个 Agent 可以使用独立的 Node runtime。</span></div><CheckCircle2 size={17}/></div>{nodeGroups.map(([major, versions]) => <div key={major} className="runtime-group"><label>Node.js {major}</label><div className="runtime-row"><select value={versions.includes(nodeVersion) ? nodeVersion : ''} onChange={e => setNodeVersion(e.target.value)}><option value="">Select version</option>{versions.slice(0, 3).map(version => <option key={version} value={version}>{version}{node?.installed.includes(version) ? ' · installed' : ''}</option>)}</select><button disabled={!nodeVersion || !versions.includes(nodeVersion) || node?.installed.includes(nodeVersion) || installingNode} onClick={installNode}><Download size={14}/>{installingNode ? 'Installing…' : 'Install'}</button></div></div>)}<small className="installed-note">{node?.installed.length ? `Installed: ${node.installed.join(', ')}` : 'No Node.js runtime installed'}</small></div>
      <div className="agent-list">{agents.map(agent => <div className="agent-card" key={agent.id}><div className="agent-icon"><Bot size={18}/></div><div className="agent-info"><div className="agent-name"><strong>{agent.name}</strong>{agent.installed ? <span className="badge installed"><CheckCircle2 size={11}/> Installed</span> : <span className="badge">Not installed</span>}</div><p>{agent.description}</p><small>{agent.requirements.length ? agent.requirements.join(' · ') : 'Standalone runtime'}</small></div>{!agent.installed && <button disabled={installingAgent === agent.id} onClick={() => installAgent(agent.id)}>{installingAgent === agent.id ? 'Installing…' : 'Install'}</button>}</div>)}</div>
    </div></div>}
  </div>;
}

createRoot(document.getElementById('root')!).render(<App />);
