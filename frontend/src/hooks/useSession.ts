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
  const activeRefreshRequestId = useRef(0);
  const activeSessionRef = useRef(active);
  const previousRunningRef = useRef(false);
  activeSessionRef.current = active;

  // Runtime status is versioned by the backend's session_runtime.updated_at.
  // Local WebSocket transitions also receive a timestamp so stale HTTP snapshots
  // can never regress a newer terminal state.
  const isStatusSnapshotNewer = useCallback(
    (incoming: Session, current?: Session) => {
      if (!current) return true;
      const incomingTime = Date.parse(incoming.status_updated_at);
      const currentTime = Date.parse(current.status_updated_at);
      if (!Number.isNaN(incomingTime) && !Number.isNaN(currentTime)) {
        return incomingTime >= currentTime;
      }
      return true;
    },
    [],
  );

  const refreshSessions = useCallback(async () => {
    const requestId = ++refreshRequestId.current;
    try {
      const latestSessions = await api<Session[]>("/sessions");
      if (requestId !== refreshRequestId.current) return;
      setSessions((currentSessions) => {
        const currentById = new Map(
          currentSessions.map((session) => [session.id, session]),
        );
        return latestSessions.map((latest) => {
          const current = currentById.get(latest.id);
          if (!current || isStatusSnapshotNewer(latest, current)) {
            return latest;
          }
          return {
            ...latest,
            status: current.status,
            status_updated_at: current.status_updated_at,
          };
        });
      });
      setError(undefined);
    } catch (error) {
      if (requestId !== refreshRequestId.current) return;
      setError(error instanceof Error ? error.message : String(error));
    }
  }, [isStatusSnapshotNewer]);

  useEffect(() => {
    refreshSessions().catch(console.error);
  }, [refreshSessions]);

  const refreshActiveSession = useCallback(
    async (sessionId: string) => {
      const requestId = ++activeRefreshRequestId.current;
      try {
        const latest = await api<Session>(`/sessions/${sessionId}`);
        if (requestId !== activeRefreshRequestId.current) return;
        setSessions((items) => {
          const current = items.find((item) => item.id === sessionId);
          if (!current || isStatusSnapshotNewer(latest, current)) {
            return current
              ? items.map((item) => (item.id === sessionId ? latest : item))
              : [latest, ...items];
          }
          return items.map((item) =>
            item.id === sessionId
              ? {
                  ...latest,
                  status: current.status,
                  status_updated_at: current.status_updated_at,
                }
              : item,
          );
        });
      } catch (error) {
        if (requestId !== activeRefreshRequestId.current) return;
        setError(error instanceof Error ? error.message : String(error));
      }
    },
    [isStatusSnapshotNewer],
  );

  // Keep polling even while locally "running". If the completion WebSocket is
  // lost, the authoritative runtime status must still recover the composer.
  useEffect(() => {
    if (!active) return;
    const timer = window.setInterval(() => {
      refreshActiveSession(active).catch(console.error);
    }, 1000);
    return () => window.clearInterval(timer);
  }, [active, refreshActiveSession]);

  const actions = useSessionActions({
    setSessions,
  });

  const sendChat = useCallback(
    async (sessionId: string, message: string) => {
      const content = message.trim();
      if (!content || activeSessionRef.current !== sessionId) return;
      setMessages((items) => [
        ...items,
        { id: crypto.randomUUID(), role: "user", content },
      ]);
      setActivity([]);
      setActivityOpen(false);
      try {
        await actions.sendMessage(sessionId, content);
      } catch (error) {
        if (activeSessionRef.current === sessionId) {
          setMessages((items) => [
            ...items,
            {
              id: crypto.randomUUID(),
              role: "error",
              content: error instanceof Error ? error.message : String(error),
            },
          ]);
        }
        throw error;
      }
    },
    [actions.sendMessage],
  );

  useSessionEvents({
    active,
    refreshSessions,
    refreshActiveSession,
    setMessages,
    setFiles,
    setStream,
    setActivity,
    setActivityOpen,
    setWorkspaceRevision,
    setError,
    setSessionStatus: (sessionId, status) => {
      const statusUpdatedAt = new Date().toISOString();
      setSessions((items) =>
        items.map((item) =>
          item.id === sessionId
            ? { ...item, status, status_updated_at: statusUpdatedAt }
            : item,
        ),
      );
    },
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

  useEffect(() => {
    const running = active
      ? sessions.find((item) => item.id === active)?.status === "running"
      : false;
    if (running && !previousRunningRef.current) setActivityOpen(true);
    if (!running && previousRunningRef.current) setActivityOpen(false);
    previousRunningRef.current = running;
  }, [active, sessions]);

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
