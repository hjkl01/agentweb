import {
  AlertTriangle,
  Bot,
  Edit3,
  LogOut,
  MessageCircle,
  MoreHorizontal,
  Pin,
  PinOff,
  Plus,
  Settings,
  Search,
  Terminal,
  Trash2,
  UserRound,
} from "lucide-react";
import { useState } from "react";
import type { Agent, Session } from "../types";

type Props = {
  agents: Agent[];
  sessions: Session[];
  active?: string;
  sessionMenu?: string;
  onNewChat: () => void;
  onSelectSession: (id: string) => void;
  onSessionMenu: (id: string) => void;
  onRenameSession: (id: string) => void;
  onTogglePin: (id: string) => void;
  onDeleteSession: (id: string) => void;
  onOpenCatalog: () => void;
  onOpenAccount: () => void;
  onLogout: () => void;
  onHome: () => void;
  onOpenTerminal: () => void;
};
export function Sidebar({
  agents,
  sessions,
  active,
  sessionMenu,
  onNewChat,
  onSelectSession,
  onSessionMenu,
  onRenameSession,
  onTogglePin,
  onDeleteSession,
  onOpenCatalog,
  onOpenAccount,
  onLogout,
  onHome,
  onOpenTerminal,
}: Props) {
  const [sessionQuery, setSessionQuery] = useState("");
  const normalizedQuery = sessionQuery.trim().toLocaleLowerCase();
  const filteredSessions = sessions
    .filter((session) => {
      if (!normalizedQuery) return true;
      const agentName =
        agents.find((agent) => agent.id === session.agent_id)?.name ||
        session.agent_id;
      return [
        session.title,
        session.workspace,
        session.agent_id,
        agentName,
        session.status,
      ].some((value) => value.toLocaleLowerCase().includes(normalizedQuery));
    })
    .sort((a, b) => Number(b.is_pinned) - Number(a.is_pinned));
  return (
    <aside className="sidebar">
      <button className="brand" onClick={onHome} aria-label="返回首页">
        <div className="brand-mark">
          <Bot size={18} />
        </div>
        <div>
          <strong>Agent Web</strong>
          <span>Agent workspace</span>
        </div>
      </button>
      <button className="new-chat" onClick={onNewChat}>
        <Plus size={16} /> New chat
      </button>
      <button
        className="settings-button terminal-nav-button"
        onClick={onOpenTerminal}
      >
        <Terminal size={16} /> Terminal
      </button>
      <div className="section-label">
        Chats{" "}
        <span>
          {normalizedQuery
            ? filteredSessions.length + "/" + sessions.length
            : sessions.length}
        </span>
      </div>
      <div className="session-search" role="search">
        <Search size={14} aria-hidden="true" />
        <input
          type="search"
          value={sessionQuery}
          onChange={(event) => setSessionQuery(event.target.value)}
          placeholder="搜索会话、工作区…"
          aria-label="搜索会话"
        />
      </div>
      <div className="session-list">
        {filteredSessions.map((session) => (
          <div
            key={session.id}
            className={`session ${active === session.id ? "active" : ""} ${session.is_pinned ? "pinned" : ""} ${session.workspace_available === false ? "workspace-missing" : ""}`}
            onClick={() => onSelectSession(session.id)}
          >
            {session.is_pinned ? (
              <Pin size={15} />
            ) : (
              <MessageCircle size={15} />
            )}
            <div className="session-main">
              <span>{session.title}</span>
              <small>
                {session.workspace_available === false ? (
                  <span
                    className="workspace-warning"
                    title="工作区目录不存在或不在允许的工作区根目录内"
                  >
                    <AlertTriangle size={12} /> 工作区不可用
                  </span>
                ) : (
                  agents.find((a) => a.id === session.agent_id)?.name ||
                  session.agent_id
                )}
              </small>
            </div>
            <div className="session-actions">
              <button
                className="icon-button session-action"
                onClick={(e) => {
                  e.stopPropagation();
                  onTogglePin(session.id);
                }}
                title={session.is_pinned ? "取消置顶" : "置顶"}
                aria-label={session.is_pinned ? "取消置顶" : "置顶"}
              >
                {session.is_pinned ? <PinOff size={14} /> : <Pin size={14} />}
              </button>
              <button
                className="icon-button session-action session-delete"
                onClick={(e) => {
                  e.stopPropagation();
                  onDeleteSession(session.id);
                }}
                title="删除会话"
                aria-label="删除会话"
              >
                <Trash2 size={14} />
              </button>
              <button
                className="icon-button session-menu"
                onClick={(e) => {
                  e.stopPropagation();
                  onSessionMenu(session.id);
                }}
                title="更多操作"
                aria-label="更多操作"
              >
                <MoreHorizontal size={15} />
              </button>
            </div>
            {sessionMenu === session.id && (
              <div className="menu" onClick={(e) => e.stopPropagation()}>
                {session.workspace_available !== false && (
                  <>
                    <button onClick={() => onTogglePin(session.id)}>
                      {session.is_pinned ? (
                        <PinOff size={14} />
                      ) : (
                        <Pin size={14} />
                      )}{" "}
                      {session.is_pinned ? "取消置顶" : "置顶"}
                    </button>
                    <button onClick={() => onRenameSession(session.id)}>
                      <Edit3 size={14} /> Rename
                    </button>
                  </>
                )}
                <button onClick={() => onDeleteSession(session.id)}>
                  <Trash2 size={14} /> Delete
                </button>
              </div>
            )}
          </div>
        ))}
        {!sessions.length && <div className="empty-sidebar">No chats yet</div>}
        {sessions.length > 0 && !filteredSessions.length && (
          <div className="empty-sidebar">没有匹配的会话</div>
        )}
      </div>
      <div className="sidebar-bottom">
        <button className="settings-button" onClick={onOpenCatalog}>
          <Settings size={16} /> Agents & runtimes
        </button>
        <button className="settings-button" onClick={onOpenAccount}>
          <UserRound size={16} /> Account & sessions
        </button>
        <button className="settings-button logout-button" onClick={onLogout}>
          <LogOut size={16} /> Sign out
        </button>
      </div>
    </aside>
  );
}
