import { useEffect } from "react";
import type { Dispatch, SetStateAction } from "react";
import { api } from "../lib/api";
import { applyAgentEvent } from "../lib/agentActivity";
import type {
  ActivityItem,
  AgentEvent,
  ChatMessage,
  FileItem,
  Session,
} from "../types";

type Options = {
  active?: string;
  refreshSessions: () => Promise<void>;
  refreshActiveSession: (sessionId: string) => Promise<void>;
  setMessages: Dispatch<SetStateAction<ChatMessage[]>>;
  setFiles: Dispatch<SetStateAction<FileItem[]>>;
  setStream: Dispatch<SetStateAction<string>>;
  setActivity: Dispatch<SetStateAction<ActivityItem[]>>;
  setActivityOpen: Dispatch<SetStateAction<boolean>>;
  setWorkspaceRevision: Dispatch<SetStateAction<number>>;
  setError: (message?: string) => void;
};

export function useSessionEvents(options: Options) {
  const { active } = options;
  useEffect(() => {
    if (!active) return;
    let disposed = false;
    let ws: WebSocket | undefined;
    let reconnectTimer: ReturnType<typeof setTimeout> | undefined;
    let reconnectDelay = 500;
    options.setStream("");
    options.setActivity([]);
    options.setActivityOpen(false);

    const loadState = async (preserveCurrentMessages = false) => {
      const [messages, files, session, activity] = await Promise.all([
        api<ChatMessage[]>(`/sessions/${active}/messages`),
        api<FileItem[]>(`/sessions/${active}/files`),
        api<Session>(`/sessions/${active}`),
        api<AgentEvent[]>(`/sessions/${active}/activity`),
      ]);
      if (disposed) return session;
      let restoredMessages = messages;
      if (session.status === "running") {
        // Clear only the latest assistant turn before replaying events buffered
        // while the HTTP snapshot was loading.
        restoredMessages = [...messages];
        let lastUserIndex = -1;
        for (let index = restoredMessages.length - 1; index >= 0; index -= 1) {
          if (restoredMessages[index].role === "user") {
            lastUserIndex = index;
            break;
          }
        }
        for (
          let index = restoredMessages.length - 1;
          index > lastUserIndex;
          index -= 1
        ) {
          if (restoredMessages[index].role === "assistant") {
            restoredMessages[index] = {
              ...restoredMessages[index],
              content: "",
            };
            break;
          }
        }
      }
      if (!preserveCurrentMessages || restoredMessages.length > 0) {
        options.setMessages(restoredMessages);
      }
      options.setFiles(files);
      options.setStream("");
      // Running sessions replay the active turn over WebSocket. Completed sessions
      // restore activity history if the backend has any.
      options.setActivity(
        session.status === "running"
          ? []
          : activity.reduce<ActivityItem[]>(
              (items, event) => applyAgentEvent(items, event),
              [],
            ),
      );
      options.setActivityOpen(session.status === "running");
      options.setError(undefined);
      return session;
    };

    const connect = () => {
      if (disposed) return;
      let snapshotReady = false;
      const pendingEvents: string[] = [];
      const protocol = location.protocol === "https:" ? "wss" : "ws";
      const connection = new WebSocket(
        `${protocol}://${location.host}/api/sessions/${active}/events`,
      );
      ws = connection;

      connection.onopen = () => {
        reconnectDelay = 500;
        options.setError(undefined);
        // Subscribe first, then fetch the HTTP snapshot. Buffer live events while
        // the snapshot loads so fast agents cannot finish in the subscription gap.
        loadState(true)
          .then((session) => {
            if (disposed || ws !== connection) return;
            snapshotReady = true;
            for (const raw of pendingEvents.splice(0)) {
              if (
                session.agent_id === "pi" &&
                session.status !== "running" &&
                isTranscriptBackedMessageEvent(raw)
              ) {
                // pi's completed transcript is already in the snapshot. Replaying its
                // deltas would duplicate the assistant text.
                continue;
              }
              handleEvent(raw, active, options);
            }
            options.refreshSessions().catch(console.error);
          })
          .catch((error) => {
            if (disposed || ws !== connection) return;
            options.setError(
              error instanceof Error ? error.message : String(error),
            );
            // Force a clean reconnect rather than leaving an open socket whose
            // events are buffered forever after a failed snapshot.
            connection.close();
          });
      };
      connection.onerror = () => {
        if (!disposed) options.setError("WebSocket 连接失败，正在重试。");
      };
      connection.onclose = () => {
        if (disposed) return;
        reconnectTimer = setTimeout(() => {
          connect();
        }, reconnectDelay);
        reconnectDelay = Math.min(reconnectDelay * 2, 8000);
      };
      connection.onmessage = (event) => {
        if (!snapshotReady) {
          // Bound memory use if the HTTP snapshot stalls. Reconnecting will trigger
          // a fresh snapshot and recover native-session-backed transcripts.
          if (pendingEvents.length >= 2048) {
            connection.close();
            return;
          }
          pendingEvents.push(event.data);
          return;
        }
        handleEvent(event.data, active, options);
      };
    };

    // Establish the event subscription before reading state; doing this in the
    // opposite order can miss an entire response from a fast Agent.
    connect();
    return () => {
      disposed = true;
      if (reconnectTimer) clearTimeout(reconnectTimer);
      ws?.close();
    };
  }, [active]);
}

function isTranscriptBackedMessageEvent(raw: string) {
  try {
    const event = JSON.parse(raw) as AgentEvent;
    return (
      event.type === "message.started" ||
      event.type === "message.delta" ||
      event.type === "message.completed"
    );
  } catch {
    return false;
  }
}

function handleEvent(raw: string, active: string, options: Options) {
  let event: AgentEvent;
  try {
    event = JSON.parse(raw) as AgentEvent;
  } catch {
    return;
  }
  const data = event.data || {};
  if (event.type === "message.started") {
    options.setActivityOpen(true);
    options.setStream("");
    options.setMessages((items) =>
      items.some((item) => item.role === "assistant" && item.content === "")
        ? items
        : [
            ...items,
            {
              id: `agent-${crypto.randomUUID()}`,
              role: "assistant",
              content: "",
            },
          ],
    );
  }
  if (event.type === "message.delta") {
    const text = String(data.text || "");
    options.setMessages((items) => {
      const target = [...items]
        .map((item, index) => ({ item, index }))
        .reverse()
        .find(({ item }) => item.role === "assistant");
      if (!target) {
        return [
          ...items,
          {
            id: `agent-${crypto.randomUUID()}`,
            role: "assistant",
            content: text,
          },
        ];
      }
      const next = [...items];
      next[target.index] = {
        ...target.item,
        content: target.item.content + text,
      };
      return next;
    });
  }
  if (event.type === "message.completed") {
    // A message can complete just before the session status changes to idle.
    // Keep activity expanded until the authoritative session-completed/error event.
    options.setStream("");
  }
  const activityEvent =
    event.type.startsWith("thinking.") ||
    event.type.startsWith("tool.") ||
    event.type.startsWith("command.") ||
    event.type.startsWith("file.") ||
    event.type === "message.started" ||
    event.type === "message.completed" ||
    event.type === "agent.error" ||
    event.type === "error" ||
    event.type === "session.completed";
  if (activityEvent) {
    if (
      event.type !== "session.completed" &&
      event.type !== "message.completed" &&
      event.type !== "agent.error" &&
      event.type !== "error"
    ) {
      options.setActivityOpen(true);
    }
    options.setActivity((items) => applyAgentEvent(items, event));
  }
  if (event.type.startsWith("file.")) {
    options.setWorkspaceRevision((value) => value + 1);
  }
  if (
    event.type === "session.completed" ||
    event.type === "agent.error" ||
    event.type === "error"
  ) {
    options.setActivityOpen(false);
    options.setWorkspaceRevision((value) => value + 1);
    options.refreshActiveSession(active).catch(console.error);
    options.refreshSessions().catch(console.error);
  }
}
