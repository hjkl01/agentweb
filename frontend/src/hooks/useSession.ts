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
  activeSessionRef.current = active;
  // Incremented only when an authoritative session-detail response is accepted.
  // This prevents an older /sessions list response from overwriting that status.
  const acceptedDetailRevision = useRef(new Map<string, number>());
  const refreshSessions = useCallback(async () => {
    const requestId = ++refreshRequestId.current;
    const detailRevisionsAtRequest = new Map(acceptedDetailRevision.current);
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
          // A detail request completed after this list request began. Keep the
          // newer detail status rather than regressing to a stale list snapshot.
          if (current && currentRevision > revisionAtRequest) {
            return { ...latest, status: current.status };
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
    try {
      const latest = await api<Session>(`/sessions/${sessionId}`);
      if (requestId !== activeRefreshRequestId.current) return;
      acceptedDetailRevision.current.set(
        sessionId,
        (acceptedDetailRevision.current.get(sessionId) ?? 0) + 1,
      );
      setSessions((items) => {
        const exists = items.some((item) => item.id === sessionId);
        return exists
          ? items.map((item) => (item.id === sessionId ? latest : item))
          : [latest, ...items];
      });
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
  const actions = useSessionActions({ setSessions });
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
