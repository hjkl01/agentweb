import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useAgentRuntime } from "./useAgentRuntime";
import { useAgentInstallation } from "./useAgentInstallation";
import { useAgentModels } from "./useAgentModels";
import { useSession } from "./useSession";
import { useWorkspace } from "./useWorkspace";
import { api } from "../lib/api";
import { sessionPath } from "../app/router";

export function useAppController(
  sessionId: string | undefined,
  navigate: (path: string, replace?: boolean) => void,
) {
  const runtime = useAgentRuntime();
  const [input, setInput] = useState("");
  const [queuedMessages, setQueuedMessages] = useState<string[]>([]);
  const queueSendingRef = useRef<string | undefined>(undefined);
  const queueSessionRef = useRef(sessionId);
  const [catalogOpen, setCatalogOpen] = useState(false);
  const [accountOpen, setAccountOpen] = useState(false);
  const [newChatOpen, setNewChatOpen] = useState(false);
  const [newChatWorkspace, setNewChatWorkspace] = useState<string>();
  const [sessionMenu, setSessionMenu] = useState<string>();
  const [focusLine, setFocusLine] = useState<number>();
  const chatRef = useRef<HTMLElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const session = useSession(sessionId);
  const workspace = useWorkspace(sessionId, session.workspaceRevision);
  const installation = useAgentInstallation(
    runtime.agents,
    runtime.nodeVersion,
    runtime.node,
    runtime.refresh,
  );
  const current = useMemo(
    () => session.sessions.find((x) => x.id === sessionId),
    [session.sessions, sessionId],
  );
  const currentAgent = useMemo(
    () => runtime.agents.find((x) => x.id === current?.agent_id),
    [runtime.agents, current?.agent_id],
  );
  const modelState = useAgentModels(current?.agent_id);
  // 文件/差异加载失败（工作区目录不存在、会话已删）只在文件面板里报，不该拦下整个会话页。
  const startupError = runtime.error || session.error || installation.error;
  const logout = async () => {
    try {
      await api("/auth/logout", { method: "POST" });
    } finally {
      window.location.href = "/";
    }
  };
  const createChat = async (
    agentId: string | undefined,
    model: string | undefined,
    workspace: string,
  ) => {
    try {
      const created = await session.createSession(agentId, model, workspace);
      navigate(sessionPath(created.id));
      setNewChatOpen(false);
      setNewChatWorkspace(undefined);
      setCatalogOpen(false);
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
    }
  };
  /** Opens the new-chat picker pre-pointed at a project from the sidebar. */
  const newChatInWorkspace = (workspace?: string) => {
    setNewChatWorkspace(workspace);
    setNewChatOpen(true);
  };
  useEffect(() => {
    queueSessionRef.current = undefined;
    setQueuedMessages([]);
  }, [sessionId]);
  const send = useCallback(async () => {
    if (!sessionId || !input.trim()) return;
    const message = input.trim();
    setInput("");
    setFocusLine(undefined);
    if (current?.status === "running") {
      queueSessionRef.current = sessionId;
      setQueuedMessages((items) => [...items, message]);
      // 排队也要立刻出现在时间线里，否则用户以为追加的任务丢了。
      session.echoPendingUser(message);
      return;
    }
    try {
      await session.sendChat(sessionId, message);
    } catch {
      // Send failures surface through the session's own error stream instead.
    }
  }, [
    sessionId,
    input,
    current?.status,
    session.sendChat,
    session.echoPendingUser,
  ]);
  useEffect(() => {
    if (
      !sessionId ||
      queueSessionRef.current !== sessionId ||
      current?.status === "running" ||
      queuedMessages.length === 0 ||
      queueSendingRef.current === sessionId
    )
      return;
    const [nextMessage, ...remaining] = queuedMessages;
    queueSendingRef.current = sessionId;
    setQueuedMessages(remaining);
    session
      .sendChat(sessionId, nextMessage)
      .catch(() => {})
      .finally(() => {
        if (queueSendingRef.current === sessionId)
          queueSendingRef.current = undefined;
      });
  }, [sessionId, current?.status, queuedMessages, session.sendChat]);
  const renameSession = async (id: string) => {
    const target = session.sessions.find((item) => item.id === id);
    const title = window.prompt("重命名会话", target?.title || "");
    if (title === null) return;
    try {
      await session.renameSession(id, title);
      setSessionMenu(undefined);
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
    }
  };
  const togglePin = async (id: string) => {
    try {
      await session.togglePin(id);
      setSessionMenu(undefined);
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
    }
  };
  const toggleArchive = async (id: string) => {
    const target = session.sessions.find((item) => item.id === id);
    if (!target) return;
    // Archiving only moves the row into a collapsed group, so no dialog; the archive group
    // itself is the way back.
    try {
      await session.setArchived(id, !target.archived_at);
      setSessionMenu(undefined);
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
    }
  };
  const deleteSession = async (id: string) => {
    if (!confirm("删除这个会话？工作区文件不会被删除。")) return;
    try {
      const next = session.sessions.find((x) => x.id !== id)?.id;
      await session.deleteSession(id);
      setSessionMenu(undefined);
      if (sessionId === id) next ? navigate(sessionPath(next)) : navigate("/");
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
    }
  };
  const deleteSessions = async (ids: string[]) => {
    if (!ids.length) return [];
    if (!confirm(`删除选中的 ${ids.length} 个会话？工作区文件不会被删除。`))
      return [];
    try {
      const failed = await session.deleteSessions(ids);
      setSessionMenu(undefined);
      if (sessionId && ids.includes(sessionId) && !failed.includes(sessionId)) {
        const next = session.sessions.find((x) => !ids.includes(x.id))?.id;
        next ? navigate(sessionPath(next)) : navigate("/");
      }
      if (failed.length)
        alert(`${failed.length} 个会话删除失败，可能仍在运行。`);
      return failed;
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
      return ids;
    }
  };
  const forkSession = async () => {
    if (!sessionId) return;
    try {
      const fork = await session.createPiFork(sessionId);
      navigate(sessionPath(fork.id));
      setSessionMenu(undefined);
    } catch (error) {
      alert(error instanceof Error ? error.message : String(error));
    }
  };
  /** Branches the session at one message and puts that message back in the editor. */
  const forkFromMessage = async (entryId: string) => {
    if (!sessionId) return;
    try {
      const fork = await session.forkFromEntry(sessionId, entryId);
      navigate(sessionPath(fork.session.id));
      setInput(fork.text || "");
      requestAnimationFrame(() => textareaRef.current?.focus());
    } catch (error) {
      alert(error instanceof Error ? error.message : String(error));
    }
  };
  const restartMissingSession = async () => {
    if (!sessionId) return;
    try {
      await session.restartMissingSession(sessionId);
      session.setMessages([]);
      session.setActivity([]);
      session.setActivityOpen(false);
    } catch (error) {
      alert(error instanceof Error ? error.message : String(error));
    }
  };
  const stop = () => {
    if (sessionId) session.interrupt(sessionId).catch(console.error);
  };
  const changeModel = (model?: string) => {
    if (sessionId) session.setModel(sessionId, model).catch(console.error);
  };
  const changeThinking = (thinkingLevel?: string) => {
    if (sessionId)
      session.setThinking(sessionId, thinkingLevel).catch(console.error);
  };
  const changeTools = (toolPreset?: string) => {
    if (sessionId) session.setTools(sessionId, toolPreset).catch(console.error);
  };
  const changeAgent = async (agentId: string) => {
    if (
      !sessionId ||
      current?.status === "running" ||
      agentId === current?.agent_id
    )
      return;
    try {
      await session.setAgent(sessionId, agentId);
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
    }
  };
  const autoResize = () => {
    const t = textareaRef.current;
    if (!t) return;
    t.style.height = "auto";
    t.style.height = `${Math.min(t.scrollHeight, 180)}px`;
  };
  const openWorkspaceFile = (path: string, line?: number) => {
    setFocusLine(line);
    workspace.openFile(path);
  };
  return {
    runtime,
    installation,
    session,
    workspace,
    current,
    currentAgent,
    modelState,
    startupError,
    input,
    setInput,
    queuedMessages,
    catalogOpen,
    setCatalogOpen,
    accountOpen,
    setAccountOpen,
    newChatOpen,
    setNewChatOpen,
    newChatWorkspace,
    newChatInWorkspace,
    sessionMenu,
    setSessionMenu,
    focusLine,
    setFocusLine,
    chatRef,
    textareaRef,
    logout,
    createChat,
    send,
    renameSession,
    togglePin,
    toggleArchive,
    deleteSession,
    deleteSessions,
    stop,
    restartMissingSession,
    forkSession,
    forkFromMessage,
    changeAgent,
    changeModel,
    changeThinking,
    changeTools,
    autoResize,
    openWorkspaceFile,
  };
}
