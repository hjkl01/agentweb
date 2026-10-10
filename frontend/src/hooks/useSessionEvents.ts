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

    const loadState = async () => {
      const [messages, files, session] = await Promise.all([
        api<ChatMessage[]>(`/sessions/${active}/messages`),
        api<FileItem[]>(`/sessions/${active}/files`),
        api<Session>(`/sessions/${active}`),
      ]);
      if (disposed) return;
      let restoredMessages = messages;
      if (session.status === "running") {
        // The backend replays the current turn. Clear only an assistant message that
        // belongs to the latest user turn, so replayed deltas replace stale partial text.
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
      options.setMessages(restoredMessages);
      options.setFiles(files);
      options.setStream("");
      options.setActivity([]);
      options.setActivityOpen(false);
      options.setError(undefined);
    };
    const syncMessages = async () => {
      try {
        const messages = await api<ChatMessage[]>(
          `/sessions/${active}/messages`,
        );
        if (!disposed) options.setMessages(messages);
      } catch (error) {
        if (!disposed)
          options.setError(
            error instanceof Error ? error.message : String(error),
          );
      }
    };
    const connect = () => {
      if (disposed) return;
      const protocol = location.protocol === "https:" ? "wss" : "ws";
      ws = new WebSocket(
        `${protocol}://${location.host}/api/sessions/${active}/events`,
      );
      ws.onopen = () => {
        reconnectDelay = 500;
        options.setError(undefined);
        // Reconcile session state in case the completion event was emitted before this socket connected.
        options.refreshSessions().catch(console.error);
      };
      ws.onerror = () => {
        if (!disposed) options.setError("WebSocket 连接失败，正在重试。");
      };
      ws.onclose = () => {
        if (disposed) return;
        reconnectTimer = setTimeout(async () => {
          try {
            await loadState();
          } catch (error) {
            options.setError(
              error instanceof Error ? error.message : String(error),
            );
          }
          options.refreshSessions().catch(console.error);
          connect();
        }, reconnectDelay);
        reconnectDelay = Math.min(reconnectDelay * 2, 8000);
      };
      ws.onmessage = (event) => handleEvent(event.data, active, options);
    };

    loadState()
      .then(() => {
        if (!disposed) connect();
      })
      .catch((error) => {
        if (!disposed)
          options.setError(
            error instanceof Error ? error.message : String(error),
          );
      });
    return () => {
      disposed = true;
      if (reconnectTimer) clearTimeout(reconnectTimer);
      ws?.close();
    };
  }, [active]);
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
      if (!target) return items;
      const next = [...items];
      next[target.index] = {
        ...target.item,
        content: target.item.content + text,
      };
      return next;
    });
  }
  if (event.type === "message.completed") {
    options.setStream("");
    options.setActivityOpen(false);
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
    if (event.type !== "session.completed" && event.type !== "message.completed" && event.type !== "agent.error" && event.type !== "error") {
      options.setActivityOpen(true);
    }
    options.setActivity((items) => applyAgentEvent(items, event));
  }
  if (event.type.startsWith("file."))
    options.setWorkspaceRevision((value) => value + 1);
  if (event.type === "session.completed" || event.type === "agent.error" || event.type === "error") {
    options.setActivityOpen(false);
    syncMessages();
    options.setWorkspaceRevision((value) => value + 1);
    options.refreshActiveSession(active).catch(console.error);
    options.refreshSessions().catch(console.error);
  }
}
