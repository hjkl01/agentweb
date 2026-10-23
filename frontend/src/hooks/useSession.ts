import { useCallback, useEffect, useRef, useState } from "react";
import type { Dispatch, SetStateAction } from "react";
import { api } from "../lib/api";
import { settlePending } from "../lib/pendingMessages";
import type { ActivityItem, ChatMessage, FileItem, Session } from "../types";
import { useSessionActions } from "./useSessionActions";
import { useSessionEvents } from "./useSessionEvents";

export function useSession(active?: string) {
  const [sessions, setSessions] = useState<Session[]>([]);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [files, setFiles] = useState<FileItem[]>([]);
  const [stream, setStream] = useState("");
  // 排队中或刚发出的用户消息：转录里出现之前一直由本地回显渲染（见 pendingMessages.ts）。
  const [pendingUser, setPendingUser] = useState<string[]>([]);
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

  const activeRunning = active
    ? sessions.find((item) => item.id === active)?.status === "running"
    : false;

  const actions = useSessionActions({
    setSessions,
  });

  const echoPendingUser = useCallback((text: string) => {
    // 队列里的消息出队时还会再调一次 sendChat，这里去重避免回显两条。
    setPendingUser((items) =>
      items.includes(text) ? items : [...items, text],
    );
  }, []);

  // 服务端整表替换转录时，把已经进转录的回显清掉；流式增量（函数式更新）不动。
  const setServerMessages: Dispatch<SetStateAction<ChatMessage[]>> = (next) => {
    if (typeof next !== "function")
      setPendingUser((items) => settlePending(items, next));
    setMessages(next);
  };

  const sendChat = useCallback(
    async (sessionId: string, message: string) => {
      const content = message.trim();
      if (!content || activeSessionRef.current !== sessionId) return;
      echoPendingUser(content);
      setActivity([]);
      setActivityOpen(false);
      try {
        await actions.sendMessage(sessionId, content);
      } catch (error) {
        // 没发出去就别留着回显。
        setPendingUser((items) => items.filter((item) => item !== content));
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
    [actions.sendMessage, echoPendingUser],
  );

  useSessionEvents({
    active,
    refreshSessions,
    refreshActiveSession,
    setMessages: setServerMessages,
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
    if (activeRunning && !previousRunningRef.current) setActivityOpen(true);
    if (!activeRunning && previousRunningRef.current) setActivityOpen(false);
    previousRunningRef.current = activeRunning;
  }, [activeRunning]);

  return {
    sessions,
    messages,
    stream,
    activity,
    activityOpen,
    workspaceRevision,
    pendingUser,
    error,
    echoPendingUser,
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
