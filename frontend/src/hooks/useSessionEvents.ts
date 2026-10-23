import { useEffect } from "react";
import type { Dispatch, SetStateAction } from "react";
import { api } from "../lib/api";
import { applyAgentEvent } from "../lib/agentActivity";
import type {
  ActivityItem,
  AgentEvent,
  BranchPoint,
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
  setBranches: Dispatch<SetStateAction<BranchPoint[]>>;
  /** 选中的分支叶节点；不传表示 pi 的默认分支。 */
  leaf?: string;
  setStream: Dispatch<SetStateAction<string>>;
  setActivity: Dispatch<SetStateAction<ActivityItem[]>>;
  setActivityOpen: Dispatch<SetStateAction<boolean>>;
  setWorkspaceRevision: Dispatch<SetStateAction<number>>;
  setError: (message?: string) => void;
  setSessionStatus: (sessionId: string, status: Session["status"]) => void;
};

/** 分支选择只是给转录请求加一个查询参数，默认分支不需要。 */
function leafQuery(leaf?: string) {
  return leaf ? `?leaf=${encodeURIComponent(leaf)}` : "";
}

export function useSessionEvents(options: Options) {
  const { active, leaf } = options;
  useEffect(() => {
    if (!active) return;
    let disposed = false;
    let ws: WebSocket | undefined;
    let reconnectTimer: ReturnType<typeof setTimeout> | undefined;
    let reconnectDelay = 500;
    // A session switch must never display the previous session's transcript.
    // loadState(true) intentionally preserves local messages during reconnects,
    // so clear session-scoped state here before opening the new subscription.
    options.setMessages([]);
    options.setFiles([]);
    options.setStream("");
    options.setActivity([]);
    options.setActivityOpen(false);

    const loadState = async (preserveCurrentMessages = false) => {
      const [messages, files, session, activity, branches] = await Promise.all([
        api<ChatMessage[]>(`/sessions/${active}/messages${leafQuery(leaf)}`),
        api<FileItem[]>(`/sessions/${active}/files`),
        api<Session>(`/sessions/${active}`),
        api<AgentEvent[]>(`/sessions/${active}/activity`),
        api<BranchPoint[]>(`/sessions/${active}/branches`).catch(() => []),
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
              parts: undefined,
            };
            break;
          }
        }
      }
      if (!preserveCurrentMessages || restoredMessages.length > 0) {
        options.setMessages(restoredMessages);
      }
      options.setFiles(files);
      options.setBranches(branches);
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
          .then(async (session) => {
            if (disposed || ws !== connection) return;
            // The messages endpoint may have just detected a deleted Pi
            // transcript and marked the session as missing. Refresh detail only
            // after that request has completed, avoiding a stale parallel snapshot
            // that would hide the recovery prompt until a later navigation.
            await options.refreshActiveSession(active);
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
      // 协议层的 pong 浏览器 JS 看不到，所以后端收到文本会用 JSON 回一个 pong：一段时间
      // 收不到任何回应说明这条连接已经是半开的（睡眠/切网不触发 onclose），主动断开，
      // 交给下面 onclose 的重连逻辑重建并重新拉一次快照。
      let pongTimer: ReturnType<typeof setTimeout> | undefined;
      const heartbeat = window.setInterval(() => {
        if (connection.readyState !== WebSocket.OPEN) return;
        connection.send('{"type":"ping"}');
        window.clearTimeout(pongTimer);
        pongTimer = window.setTimeout(
          () => connection.close(),
          PONG_TIMEOUT_MS,
        );
      }, HEARTBEAT_MS);
      connection.onerror = () => {
        if (!disposed) options.setError("WebSocket 连接失败，正在重试。");
      };
      connection.onclose = () => {
        window.clearInterval(heartbeat);
        window.clearTimeout(pongTimer);
        if (disposed) return;
        reconnectTimer = setTimeout(() => {
          connect();
        }, reconnectDelay);
        reconnectDelay = Math.min(reconnectDelay * 2, 8000);
      };
      connection.onmessage = (event) => {
        // 任何一条消息都说明链路是活的。
        window.clearTimeout(pongTimer);
        // WebSocket close is asynchronous: a queued message from the previous
        // session can still arrive after cleanup. Never let an obsolete socket
        // mutate the newly selected session's UI state.
        if (disposed || ws !== connection) return;
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
  }, [active, leaf]);
}

/** 心跳间隔：空闲会话也要定期确认链路，但走的是 WebSocket 不是 HTTP。 */
const HEARTBEAT_MS = 15_000;
/** 心跳发出后多久收不到任何消息就判定这条连接已死。 */
const PONG_TIMEOUT_MS = 10_000;

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
    // The completion/error event is emitted only after the backend has finalized
    // the runtime lease. Reflect it immediately so the composer switches from
    // Stop to Send without waiting for the HTTP reconciliation request.
    options.setSessionStatus(
      active,
      event.type === "session.completed" ? "idle" : "error",
    );
    options.setActivityOpen(false);
    options.setWorkspaceRevision((value) => value + 1);
    // Pi keeps the conversation in its native JSONL transcript. Re-read it so
    // structured parts (thinking/tool calls/results) replace the flat stream.
    // Non-Pi agents return an empty list here, so their streamed transcript is
    // preserved instead of being cleared.
    void api<ChatMessage[]>(`/sessions/${active}/messages`)
      .then((messages) => {
        if (messages.length) options.setMessages(messages);
      })
      .catch(console.error);
    options.refreshActiveSession(active).catch(console.error);
    options.refreshSessions().catch(console.error);
  }
}
