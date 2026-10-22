import {
  AlertTriangle,
  Archive,
  ArchiveRestore,
  Bot,
  Check,
  ChevronRight,
  Edit3,
  Folder,
  GitBranch,
  ExternalLink,
  ListChecks,
  LogOut,
  SlidersHorizontal,
  MessageCircle,
  MoreHorizontal,
  Pin,
  PinOff,
  Plus,
  Settings,
  Search,
  Terminal,
  Clock3,
  Trash2,
  UserRound,
  X,
} from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../lib/api";
import { sessionPath } from "../app/router";
import { familyRows } from "../lib/sessionFamily";
import { ARCHIVED_KEY, PINNED_KEY, groupSessions } from "../lib/sessionGroups";
import type {
  Agent,
  Session,
  SessionSearchHit,
  SessionSearchResponse,
} from "../types";

// Menu is four rows tall; flip it above the row when it would be clipped.
const MENU_HEIGHT = 160;
// 折叠状态只存在浏览器里，和会话分组本身无关。
const GROUP_STORAGE_KEY = "agentweb.sessionGroups";

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
  onToggleArchive: (id: string) => void;
  onDeleteSession: (id: string) => void;
  onDeleteSessions: (ids: string[]) => Promise<string[]>;
  onNewChatInWorkspace: (workspace: string) => void;
  onOpenCatalog: () => void;
  onOpenAccount: () => void;
  onLogout: () => void;
  onHome: () => void;
  onOpenTerminal: () => void;
  onOpenScheduledTasks: () => void;
  onOpenSettings: () => void;
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
  onToggleArchive,
  onDeleteSession,
  onDeleteSessions,
  onNewChatInWorkspace,
  onOpenCatalog,
  onOpenAccount,
  onLogout,
  onHome,
  onOpenTerminal,
  onOpenScheduledTasks,
  onOpenSettings,
}: Props) {
  const [sessionQuery, setSessionQuery] = useState("");
  const [selectMode, setSelectMode] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(() => new Set());
  const [hits, setHits] = useState<SessionSearchHit[]>([]);
  const [hitsTruncated, setHitsTruncated] = useState(false);
  const [hitsSearching, setHitsSearching] = useState(false);
  const [hitsFailed, setHitsFailed] = useState(false);
  // Conversation content is searched by the backend (it lives in the Agent's own session
  // store), while the list below keeps filtering titles instantly.
  useEffect(() => {
    const query = sessionQuery.trim();
    if (!query) {
      setHits([]);
      setHitsFailed(false);
      setHitsSearching(false);
      return;
    }
    const controller = new AbortController();
    setHitsSearching(true);
    setHitsFailed(false);
    const timer = setTimeout(async () => {
      try {
        const response = await api<SessionSearchResponse>(
          `/sessions/search?q=${encodeURIComponent(query)}`,
          { signal: controller.signal },
        );
        if (controller.signal.aborted) return;
        setHits(response.results);
        setHitsTruncated(response.truncated);
      } catch {
        if (controller.signal.aborted) return;
        setHits([]);
        setHitsFailed(true);
      } finally {
        if (!controller.signal.aborted) setHitsSearching(false);
      }
    }, 300);
    return () => {
      clearTimeout(timer);
      controller.abort();
    };
  }, [sessionQuery]);
  const [menuUp, setMenuUp] = useState(false);
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>(() => {
    try {
      return JSON.parse(localStorage.getItem(GROUP_STORAGE_KEY) || "{}");
    } catch {
      return {};
    }
  });
  const toggleGroup = (key: string) =>
    setCollapsed((prev) => {
      const next = { ...prev, [key]: !prev[key] };
      try {
        localStorage.setItem(GROUP_STORAGE_KEY, JSON.stringify(next));
      } catch {
        // Ignore unavailable storage (private mode).
      }
      return next;
    });
  // Archived sessions start collapsed; an explicit stored choice wins afterwards.
  const isCollapsed = (key: string) => collapsed[key] ?? key === ARCHIVED_KEY;
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
  const groups = groupSessions(filteredSessions);
  const allSelected =
    filteredSessions.length > 0 &&
    filteredSessions.every((session) => selected.has(session.id));
  const exitSelect = () => {
    setSelectMode(false);
    setSelected(new Set());
  };
  const toggleSelected = (id: string) =>
    setSelected((prev) => {
      const next = new Set(prev);
      next.has(id) ? next.delete(id) : next.add(id);
      return next;
    });
  const deleteSelected = async () => {
    const ids = [...selected];
    const failed = await onDeleteSessions(ids);
    // Keep whatever the backend refused (still running) selected for a retry.
    failed.length ? setSelected(new Set(failed)) : exitSelect();
  };

  // 分支关系在会话列表里，前端即可把家族排成父子。
  return (
    <aside className="sidebar">
      <button className="brand" onClick={onHome} aria-label="返回首页">
        <div className="brand-mark">
          <Bot size={18} />
        </div>
        <div>
          <strong>Agent Web</strong>
          <span>Agent 工作台</span>
        </div>
      </button>
      <button className="new-chat" onClick={onNewChat}>
        <Plus size={16} /> New chat
      </button>
      <div className="sidebar-tools">
        <button
          className="settings-button terminal-nav-button"
          onClick={onOpenTerminal}
        >
          <Terminal size={16} /> Terminal
        </button>
        <button
          className="settings-button terminal-nav-button"
          onClick={onOpenScheduledTasks}
        >
          <Clock3 size={16} /> 定时任务
        </button>
      </div>
      <div className="section-label">
        Chats
        <span>
          {normalizedQuery
            ? filteredSessions.length + "/" + sessions.length
            : sessions.length}
        </span>
        <button
          type="button"
          className={`icon-button session-select-toggle${selectMode ? " on" : ""}`}
          onClick={() => (selectMode ? exitSelect() : setSelectMode(true))}
          title="批量选择会话"
          aria-label="批量选择会话"
          aria-pressed={selectMode}
        >
          <ListChecks size={14} />
        </button>
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
      {selectMode && (
        <div className="session-bulk">
          <button
            type="button"
            onClick={() =>
              setSelected(
                allSelected
                  ? new Set()
                  : new Set(filteredSessions.map((s) => s.id)),
              )
            }
          >
            {allSelected ? "取消全选" : "全选"}
          </button>
          <span>已选 {selected.size}</span>
          <button
            type="button"
            className="danger"
            disabled={!selected.size}
            onClick={deleteSelected}
          >
            <Trash2 size={13} /> 删除
          </button>
          <button type="button" onClick={exitSelect} aria-label="退出批量选择">
            <X size={13} />
          </button>
        </div>
      )}
      <div className="session-list">
        {normalizedQuery && (
          <div className="session-hits">
            <div className="session-hits-head">
              内容匹配
              <span>
                {hitsSearching
                  ? "搜索中…"
                  : hitsFailed
                    ? "搜索失败"
                    : `${hits.length}${hitsTruncated ? "+" : ""}`}
              </span>
            </div>
            {hits.map((hit) => (
              <button
                key={`${hit.session.id}:${hit.entry_id}`}
                type="button"
                className="session-hit"
                onClick={() => onSelectSession(hit.session.id)}
                title={hit.session.workspace}
              >
                <span className="session-hit-title">
                  {hit.session.title}
                  {hit.session.archived_at ? " · 已归档" : ""}
                </span>
                <span className="session-hit-snippet">
                  {hit.before}
                  <mark>{hit.matched}</mark>
                  {hit.after}
                </span>
              </button>
            ))}
            {hitsTruncated && (
              <div className="session-hits-note">
                只搜索了较新的会话，可用更具体的关键词
              </div>
            )}
          </div>
        )}
        {groups.map((group) => (
          <div className="session-group" key={group.key}>
            <div className="session-group-head">
              <button
                type="button"
                className="session-group-toggle"
                onClick={() => toggleGroup(group.key)}
                aria-expanded={!isCollapsed(group.key)}
              >
                <ChevronRight
                  size={11}
                  className={`session-group-chevron${isCollapsed(group.key) ? "" : " open"}`}
                />
                {group.key === PINNED_KEY ? (
                  <Pin size={12} />
                ) : group.key === ARCHIVED_KEY ? (
                  <Archive size={12} />
                ) : (
                  <Folder size={12} />
                )}
                <span className="session-group-name" title={group.label}>
                  {group.label}
                </span>
                <span className="session-group-count">
                  {group.sessions.length}
                </span>
              </button>
              {group.key !== PINNED_KEY && group.key !== ARCHIVED_KEY && (
                <div className="session-group-actions">
                  <button
                    type="button"
                    className="icon-button session-group-action"
                    title="在这个项目里新建会话"
                    aria-label="在这个项目里新建会话"
                    onClick={() => onNewChatInWorkspace(group.key)}
                  >
                    <Plus size={12} />
                  </button>
                  <button
                    type="button"
                    className="icon-button session-group-action"
                    title="删除这个项目的全部会话"
                    aria-label="删除这个项目的全部会话"
                    onClick={() =>
                      onDeleteSessions(group.sessions.map((s) => s.id))
                    }
                  >
                    <Trash2 size={12} />
                  </button>
                </div>
              )}
            </div>
            {!isCollapsed(group.key) &&
              familyRows(group.sessions).map(({ session, depth }) => (
                <div
                  key={session.id}
                  className={`session ${active === session.id ? "active" : ""} ${session.is_pinned ? "pinned" : ""} ${session.workspace_available === false ? "workspace-missing" : ""} ${selected.has(session.id) ? "selected" : ""}`}
                  style={
                    depth ? { paddingLeft: `${14 + depth * 16}px` } : undefined
                  }
                  onClick={() =>
                    selectMode
                      ? toggleSelected(session.id)
                      : onSelectSession(session.id)
                  }
                >
                  {selectMode ? (
                    <span
                      className={`session-check${selected.has(session.id) ? " on" : ""}`}
                      aria-hidden="true"
                    >
                      {selected.has(session.id) && <Check size={11} />}
                    </span>
                  ) : session.archived_at ? (
                    <Archive size={13} />
                  ) : depth ? (
                    <GitBranch size={13} />
                  ) : session.is_pinned ? (
                    <Pin size={13} />
                  ) : (
                    <MessageCircle size={13} />
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
                  {!selectMode && (
                    <div className="session-actions">
                      <button
                        className="icon-button session-menu"
                        onClick={(e) => {
                          e.stopPropagation();
                          const list = e.currentTarget
                            .closest(".session-list")!
                            .getBoundingClientRect();
                          const row = e.currentTarget.getBoundingClientRect();
                          setMenuUp(row.bottom + MENU_HEIGHT > list.bottom);
                          onSessionMenu(session.id);
                        }}
                        title="对话操作"
                        aria-label="打开对话操作"
                        aria-expanded={sessionMenu === session.id}
                      >
                        <MoreHorizontal size={13} />
                      </button>
                    </div>
                  )}
                  {sessionMenu === session.id && (
                    <div
                      className={`menu${menuUp ? " up" : ""}`}
                      onClick={(e) => e.stopPropagation()}
                    >
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
                            <Edit3 size={14} /> 重命名
                          </button>
                        </>
                      )}
                      <button
                        onClick={() => {
                          window.open(sessionPath(session.id), "_blank");
                          onSessionMenu(undefined);
                        }}
                      >
                        <ExternalLink size={14} /> 在新标签打开
                      </button>
                      <button onClick={() => onToggleArchive(session.id)}>
                        {session.archived_at ? (
                          <ArchiveRestore size={14} />
                        ) : (
                          <Archive size={14} />
                        )}{" "}
                        {session.archived_at ? "取消归档" : "归档"}
                      </button>
                      <button onClick={() => onDeleteSession(session.id)}>
                        <Trash2 size={14} /> 删除会话
                      </button>
                    </div>
                  )}
                </div>
              ))}
          </div>
        ))}
        {!sessions.length && <div className="empty-sidebar">还没有会话</div>}
        {sessions.length > 0 && !filteredSessions.length && !hits.length && (
          <div className="empty-sidebar">没有匹配的会话</div>
        )}
      </div>
      <div className="sidebar-bottom">
        <button className="settings-button" onClick={onOpenCatalog}>
          <Settings size={16} /> Agent 与运行时
        </button>
        <button className="settings-button" onClick={onOpenSettings}>
          <SlidersHorizontal size={16} /> 设置
        </button>
        <button className="settings-button" onClick={onOpenAccount}>
          <UserRound size={16} /> 账号与会话
        </button>
        <button className="settings-button logout-button" onClick={onLogout}>
          <LogOut size={16} /> 退出登录
        </button>
      </div>
    </aside>
  );
}
