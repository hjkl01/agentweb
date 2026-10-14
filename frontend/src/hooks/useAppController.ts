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
  const queueSendingRef = useRef(false);
  const queueSessionRef = useRef(sessionId);
  const [catalogOpen, setCatalogOpen] = useState(false);
  const [accountOpen, setAccountOpen] = useState(false);
  const [newChatOpen, setNewChatOpen] = useState(false);
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
  const startupError =
    runtime.error || session.error || workspace.error || installation.error;
  const logout = async () => {
    try {
      await api("/auth/logout", { method: "POST" });
    } finally {
      window.location.href = "/";
    }
  };
  const createChat = async (agentId: string | undefined, model: string | undefined, workspace: string) => {
    try {
      const created = await session.createSession(agentId, model, workspace);
      navigate(sessionPath(created.id));
      setNewChatOpen(false);
      setCatalogOpen(false);
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
    }
  };
  useEffect(() => {
    queueSessionRef.current = sessionId;
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
      return;
    }
    try {
      await session.sendChat(sessionId, message);
    } catch {}
  }, [sessionId, input, current?.status, session.sendChat]);
  useEffect(() => {
    if (
      !sessionId ||
      queueSessionRef.current !== sessionId ||
      current?.status === "running" ||
      queuedMessages.length === 0 ||
      queueSendingRef.current
    ) return;
    const [nextMessage, ...remaining] = queuedMessages;
    queueSendingRef.current = true;
    setQueuedMessages(remaining);
    session.sendChat(sessionId, nextMessage)
      .catch(() => {})
      .finally(() => { queueSendingRef.current = false; });
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
    deleteSession,
    stop,
    restartMissingSession,
    changeAgent,
    changeModel,
    autoResize,
    openWorkspaceFile,
  };
}
