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
  // Keep async callbacks from a previous session from mutating the newly active
  // session's transcript after navigation.
  const activeSessionRef = useRef(active);
  const previousRunningRef = useRef(false);
  activeSessionRef.current = active;
  // Incremented only when an authoritative session-detail response is accepted.
  // This prevents an older /sessions list response from overwriting that status.
  const acceptedDetailRevision = useRef(new Map<string, number>());
  // Monotonic status revisions protect optimistic/event-driven status changes from
  // HTTP responses that started before those changes and complete afterwards.
  const statusRevision = useRef(new Map<string, number>());
  const isStatusSnapshotNewer = (incoming: Session, current?: Session) => {
    if (!current) return true;
    const incomingTime = Date.parse(incoming.status_updated_at);
    const currentTime = Date.parse(current.status_updated_at);
    if (!Number.isNaN(incomingTime) && !Number.isNaN(currentTime)) {
      return incomingTime >= currentTime;
    }
    return true;
  };
  const bumpStatusRevision = useCallback((sessionId: string) => {
    const next = (statusRevision.current.get(sessionId) ?? 0) + 1;
    statusRevision.current.set(sessionId, next);
    return next;
  }, []);
  const refreshSessions = useCallback(async () => {
    const requestId = ++refreshRequestId.current;
    const detailRevisionsAtRequest = new Map(acceptedDetailRevision.current);
    const statusRevisionsAtRequest = new Map(statusRevision.current);
    try {
      const latestSessions = await api<Session[]>("/sessions");
      if (requestId !== refreshRequestId.current) return;
      setSessions((currentSessions) => {
        const currentById = new Map(
          currentSessions.map((session) => [session.id, session]),
        );
        return latestSessions.map((latest) => {
          const current = currentById.get(latest.id);
          const revisionAtRequest =
            detailRevisionsAtRequest.get(latest.id) ?? 0;
          const currentRevision =
            acceptedDetailRevision.current.get(latest.id) ?? 0;
          const currentStatusRevision = statusRevision.current.get(latest.id) ?? 0;
          const statusChangedAfterRequest =
            currentStatusRevision > (statusRevisionsAtRequest.get(latest.id) ?? 0);
          // A detail request or local status transition completed after this list
          // request began. Keep the newer status rather than regressing to a stale
          // list snapshot.
          if (current && (currentRevision > revisionAtRequest || statusChangedAfterRequest || !isStatusSnapshotNewer(latest, current))) {
            return { ...latest, status: current.status, status_updated_at: current.status_updated_at };
          }
          return latest;
        });
      });
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
    const requestId = ++activeRefreshRequestId.current;
    const statusRevisionAtRequest = statusRevision.current.get(sessionId) ?? 0;
    try {
      const latest = await api<Session>(`/sessions/${sessionId}`);
      if (requestId !== activeRefreshRequestId.current) return;
      acceptedDetailRevision.current.set(
        sessionId,
        (acceptedDetailRevision.current.get(sessionId) ?? 0) + 1,
      );
      const statusChangedAfterRequest =
        (statusRevision.current.get(sessionId) ?? 0) > statusRevisionAtRequest;
      setSessions((items) => {
        const current = items.find((item) => item.id === sessionId);
        if (!current) return [latest, ...items];
        if (statusChangedAfterRequest || !isStatusSnapshotNewer(latest, current)) {
          return items.map((item) =>
            item.id === sessionId
              ? { ...latest, status: item.status, status_updated_at: current.status_updated_at }
              : item,
          );
        }
        return items.map((item) => (item.id === sessionId ? latest : item));
      });;
    } catch (error) {
      if (requestId !== activeRefreshRequestId.current) return;
      setError(error instanceof Error ? error.message : String(error));
    }
  }, []);
  // Poll the authoritative session detail while a chat is open. Do not make
  // polling conditional on the locally cached "running" status: if the completion
  // WebSocket event is lost, that stale status would otherwise disable recovery
  // exactly when it is needed and leave the composer in Stop mode forever.
  useEffect(() => {
    if (!active) return;
    const timer = window.setInterval(() => {
      refreshActiveSession(active).catch(console.error);
    }, 1000);
    return () => window.clearInterval(timer);
  }, [active, refreshActiveSession]);
  const actions = useSessionActions({
    setSessions,
    markStatusChanged: bumpStatusRevision,
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
        bumpStatusRevision(sessionId);
        await actions.sendMessage(sessionId, content);
      } catch (error) {
        // The request may fail after the user has navigated to another session.
        // Do not append the old session's error to the new session's transcript.
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
    [actions.sendMessage, bumpStatusRevision],
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
      bumpStatusRevision(sessionId);
      setSessions((items) =>
        items.map((item) => (item.id === sessionId ? { ...item, status } : item)),
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
    const running = active ? sessions.find((item) => item.id === active)?.status === "running" : false;
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
