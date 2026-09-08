import { Bot, MessageCircle, MoreHorizontal, Plus, Settings, Trash2 } from 'lucide-react';
import type { Agent, Session } from '../types';

type Props = {
  agents: Agent[];
  sessions: Session[];
  active?: string;
  sessionMenu?: string;
  onNewChat: () => void;
  onSelectSession: (id: string) => void;
  onSessionMenu: (id: string) => void;
  onDeleteSession: (id: string) => void;
  onOpenCatalog: () => void;
};

export function Sidebar({ agents, sessions, active, sessionMenu, onNewChat, onSelectSession, onSessionMenu, onDeleteSession, onOpenCatalog }: Props) {
  return (
    <aside className="sidebar">
      <div className="brand">
        <div className="brand-mark"><Bot size={18} /></div>
        <div><strong>Agent Web</strong><span>Agent workspace</span></div>
      </div>

      <button className="new-chat" onClick={onNewChat}><Plus size={16} /> New chat</button>
      <div className="section-label">Chats <span>{sessions.length}</span></div>

      <div className="session-list">
        {sessions.map(session => (
          <div key={session.id} className={`session ${active === session.id ? 'active' : ''}`} onClick={() => onSelectSession(session.id)}>
            <MessageCircle size={15} />
            <div className="session-main">
              <span>{session.title}</span>
              <small>{agents.find(agent => agent.id === session.agent_id)?.name || session.agent_id}</small>
            </div>
            <button className="icon-button session-menu" onClick={event => { event.stopPropagation(); onSessionMenu(session.id); }}>
              <MoreHorizontal size={15} />
            </button>
            {sessionMenu === session.id && (
              <div className="menu" onClick={event => event.stopPropagation()}>
                <button onClick={() => onDeleteSession(session.id)}><Trash2 size={14} /> Delete</button>
              </div>
            )}
          </div>
        ))}
        {!sessions.length && <div className="empty-sidebar">No chats yet</div>}
      </div>

      <div className="sidebar-bottom">
        <button className="settings-button" onClick={onOpenCatalog}><Settings size={16} /> Agents & runtimes</button>
      </div>
    </aside>
  );
}