import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../lib/api";
import type { ActivityItem, ChatMessage, FileItem, Session } from "../types";
import { useSessionActions } from "./useSessionActions";
import { useSessionEvents } from "./useSessionEvents";

export function useSession(active?: string) {
  const [sessions, setSessions] = useState<Session[]>([]);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [files, setFiles] = useState<FileItem[]>([]);
  const [stream, setStream] = useState("");
  const [activity, setActivity] = useState<ActivityItem[]>([]);
  const [activityOpen, setActivityOpen] = useState(false);
  const [workspaceRevision, setWorkspaceRevision] = useState(0);
  const [error, setError] = useState<string>();
  const refreshRequestId = useRef(0);
  const refreshSessions = useCallback(async () => {
    const requestId = ++refreshRequestId.current;
    try {
      const latestSessions = await api<Session[]>("/sessions");
      // Multiple refreshes can overlap (polling, WebSocket completion, reconnect).
      // Ignore older responses so a stale "running" result cannot overwrite "idle".
      if (requestId !== refreshRequestId.current) return;
      setSessions(latestSessions);
      setError(undefined);
    } catch (error) {
      if (requestId !== refreshRequestId.current) return;
      setError(error instanceof Error ? error.message : String(error));
    }
  }, []);
  useEffect(() => {
    refreshSessions().catch(console.error);
  }, [refreshSessions]);
  const refreshActiveSession = useCallback(async (sessionId: string) => {
    try {
      const latest = await api<Session>(`/sessions/${sessionId}`);
      setSessions((items) => {
        const exists = items.some((item) => item.id === sessionId);
        return exists
          ? items.map((item) => (item.id === sessionId ? latest : item))
          : [latest, ...items];
      });
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error));
    }
  }, []);
  const activeStatus = sessions.find((item) => item.id === active)?.status;
  useEffect(() => {
    if (!active || activeStatus !== "running") return;
    // Poll the authoritative session detail, not only the session list.
    // This recovers even if the completion WebSocket event is missed.
    const timer = window.setInterval(() => {
      refreshActiveSession(active).catch(console.error);
    }, 1000);
    return () => window.clearInterval(timer);
  }, [active, activeStatus, refreshActiveSession]);
  const actions = useSessionActions({ setSessions });
  const sendChat = useCallback(
    async (sessionId: string, message: string) => {
      const content = message.trim();
      if (!content) return;
      setMessages((items) => [
        ...items,
        { id: crypto.randomUUID(), role: "user", content },
      ]);
      setActivity([]);
      setActivityOpen(false);
      try {
        await actions.sendMessage(sessionId, content);
      } catch (error) {
        setMessages((items) => [
          ...items,
          {
            id: crypto.randomUUID(),
            role: "error",
            content: error instanceof Error ? error.message : String(error),
          },
        ]);
        throw error;
      }
    },
    [actions.sendMessage],
  );
  useSessionEvents({
    active,
    refreshSessions,
    setMessages,
    setFiles,
    setStream,
    setActivity,
    setActivityOpen,
    setWorkspaceRevision,
    setError,
  });
  useEffect(() => {
    if (!active) {
      setMessages([]);
      setFiles([]);
      setStream("");
      setActivity([]);
      setActivityOpen(false);
      setWorkspaceRevision(0);
    }
  }, [active]);
  return {
    sessions,
    messages,
    stream,
    activity,
    activityOpen,
    workspaceRevision,
    error,
    setMessages,
    setFiles,
    setActivity,
    setActivityOpen,
    refreshSessions,
    refreshActiveSession,
    sendChat,
    ...actions,
  };
}
