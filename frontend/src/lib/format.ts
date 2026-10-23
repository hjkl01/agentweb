/** Small pure formatters shared by the chat transcript. */

import type { ChatMessage } from "../types";

/** `850` → `850ms`, `4200` → `4.2s`, `95000` → `1m35s`. */
export function formatDuration(ms?: number): string | undefined {
  if (ms === undefined || !Number.isFinite(ms) || ms < 0) return undefined;
  if (ms < 1000) return `${Math.round(ms)}ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(ms < 10_000 ? 1 : 0)}s`;
  const seconds = Math.round((ms % 60_000) / 1000);
  return `${Math.floor(ms / 60_000)}m${seconds}s`;
}

/** 快过这个耗时的工具不显示时间：每次快速读取都写 0.0s，只是噪音。（pi-web 同值） */
export const TOOL_DURATION_MIN_MS = 100;

/** 工具耗时，按 pi CLI 的 `Took` 写法：2.4s → 3m 5s → 1h 2m 5s。 */
export function formatToolDuration(ms?: number): string | undefined {
  if (ms === undefined || !Number.isFinite(ms) || ms < TOOL_DURATION_MIN_MS) {
    return undefined;
  }
  const total = Math.round(ms / 1000);
  if (total < 60) return `${(ms / 1000).toFixed(1)}s`;
  const seconds = total % 60;
  const minutes = Math.floor(total / 60);
  if (minutes < 60) return `${minutes}m ${seconds}s`;
  return `${Math.floor(minutes / 60)}h ${minutes % 60}m ${seconds}s`;
}

/** Keys that identify a tool call at a glance, in order of usefulness. */
const SUMMARY_KEYS = [
  "command",
  "path",
  "file_path",
  "filePath",
  "pattern",
  "query",
  "url",
  "prompt",
];

/** One-line digest of a tool call's arguments for the collapsed header. */
export function toolSummary(args: unknown): string | undefined {
  if (typeof args === "string")
    return args.replace(/\s+/g, " ").trim().slice(0, 120) || undefined;
  if (!args || typeof args !== "object") return undefined;
  const record = args as Record<string, unknown>;
  for (const key of SUMMARY_KEYS) {
    const value = record[key];
    if (typeof value === "string" && value.trim())
      return value.replace(/\s+/g, " ").trim().slice(0, 120);
  }
  return undefined;
}

/** 参数表的一行。（pi-web tool-fields.ts） */
export type ToolField = {
  key: string;
  value: string;
  multiline: boolean;
};

/** 参数是「一排标量」时排成两列表；只要有一个值是嵌套对象/数组/空值，就整体退回 JSON 视图。 */
export function toolFieldsOfInput(input: unknown): ToolField[] | undefined {
  if (!input || typeof input !== "object" || Array.isArray(input)) {
    return undefined;
  }
  const entries = Object.entries(input as Record<string, unknown>);
  if (!entries.length) return undefined;
  const fields: ToolField[] = [];
  for (const [key, raw] of entries) {
    if (raw === null || raw === undefined || typeof raw === "object") {
      return undefined;
    }
    const value = typeof raw === "string" ? raw : String(raw);
    if (!value.trim()) return undefined;
    fields.push({ key, value, multiline: value.includes("\n") });
  }
  return fields;
}

export type DiffLineKind = "hunk" | "gap" | "add" | "remove" | "context";

/** Split the diff fragment Pi attaches to `edit` results into renderable rows. */
export function diffLines(
  diff: string,
): { kind: DiffLineKind; text: string }[] {
  return diff.split("\n").map((text) => {
    if (text.startsWith("@@")) return { kind: "hunk" as const, text };
    if (text.trimStart().startsWith("..."))
      return { kind: "gap" as const, text };
    if (text.startsWith("+")) return { kind: "add" as const, text };
    if (text.startsWith("-")) return { kind: "remove" as const, text };
    return { kind: "context" as const, text };
  });
}

/* 转录分组：把「思考 → 工具调用 → 工具结果」合成一段「执行过程」。
 *
 * 这样做的原因：一次真实回答往往有 5~10 次工具调用，逐条渲染就会得到一墙带边框的
 * 卡片、外加每条工具结果一个头像行。分组后每段只占一个标题行 + 若干紧凑步骤行。
 */

export type ActivityRow = {
  key: string;
  kind: "thinking" | "tool" | "note";
  /** 工具名，或留空（思考/说明行）。 */
  name?: string;
  /** 工具参数的一行摘要。 */
  summary?: string;
  args?: unknown;
  text?: string;
  callId?: string;
  /** 挂回来的工具结果（按 `tool_call_id` 配对）。 */
  result?: ChatMessage;
};

export type TranscriptEntry =
  | { kind: "message"; message: ChatMessage }
  | { kind: "activity"; key: string; rows: ActivityRow[] };

/** 属于执行过程的消息：工具结果，以及带工具调用的助手轮次。
 *  失败的助手轮次（`stop_reason === "error"`）要单独显示，不能藏进执行过程里。 */
export function isActivityMessage(message: ChatMessage): boolean {
  if (message.role === "toolResult") return true;
  if (message.role !== "assistant" || message.stop_reason === "error")
    return false;
  return (message.parts ?? []).some((part) => part.type === "toolCall");
}

function buildActivityRows(run: ChatMessage[]): ActivityRow[] {
  const rows: ActivityRow[] = [];
  const byCall = new Map<string, ActivityRow>();
  for (const message of run) {
    if (message.role === "toolResult") {
      // 结果按 tool_call_id 挂回那次调用；对不上（历史被窗口截断、老转录没有 id）
      // 就单独成行，输出仍然可见。
      const row = message.tool_call_id
        ? byCall.get(message.tool_call_id)
        : undefined;
      if (row) row.result = message;
      else
        rows.push({
          key: message.id,
          kind: "tool",
          name: message.tool_name || "工具结果",
          result: message,
        });
      continue;
    }
    (message.parts ?? []).forEach((part, index) => {
      if (part.type === "thinking" && part.text?.trim()) {
        rows.push({
          key: `${message.id}:${index}`,
          kind: "thinking",
          text: part.text,
        });
      } else if (part.type === "toolCall") {
        const row: ActivityRow = {
          key: `${message.id}:${index}`,
          kind: "tool",
          name: part.name || "tool",
          summary: toolSummary(part.arguments),
          args: part.arguments,
          callId: part.tool_call_id,
        };
        if (row.callId) byCall.set(row.callId, row);
        rows.push(row);
      } else if (part.type === "text" && part.text?.trim()) {
        // 工具调用之间的过程说明，属于执行过程而不属于最终回答。
        rows.push({
          key: `${message.id}:${index}`,
          kind: "note",
          text: part.text,
        });
      }
    });
  }
  return rows;
}

/** 把转录切成普通消息与执行过程：连续的工具循环消息合并成一段。 */
export function groupTranscript(messages: ChatMessage[]): TranscriptEntry[] {
  const entries: TranscriptEntry[] = [];
  let run: ChatMessage[] = [];
  const flush = () => {
    if (!run.length) return;
    entries.push({
      kind: "activity",
      key: run[0].id,
      rows: buildActivityRows(run),
    });
    run = [];
  };
  for (const message of messages) {
    if (isActivityMessage(message)) {
      run.push(message);
    } else {
      flush();
      entries.push({ kind: "message", message });
    }
  }
  flush();
  return entries;
}

/** 工具名 → 中文短标签。查不到就原样显示英文名：宁可露出原名，也不要猜一个错的动词。 */
const TOOL_LABELS: Record<string, string> = {
  bash: "运行命令",
  read: "读取文件",
  write: "写入文件",
  edit: "修改文件",
  patch: "修改文件",
  grep: "搜索代码",
  glob: "查找文件",
  web_search: "联网搜索",
  fetch_content: "抓取网页",
  compress: "压缩上下文",
  decompress: "展开上下文",
  search_context: "检索上下文",
  subagent: "委派任务",
  task: "委派任务",
  todo: "更新待办",
  memory_read: "读取记忆",
  memory_write: "写入记忆",
  memory_search: "检索记忆",
  scratchpad: "记录备忘",
  ask_user_question: "询问用户",
};

export function toolLabel(name?: string): string {
  if (!name) return "工具";
  return TOOL_LABELS[name] ?? name;
}

/** 执行过程标题行右侧的事实：`运行命令 ×3 · 读取文件 · 12s`，以及出错/截断/运行中的提示。 */
export function activityFacts(rows: ActivityRow[]): {
  tools: string;
  durationMs: number;
  failed: number;
  truncated: number;
  pending: number;
} {
  const counts = new Map<string, number>();
  let durationMs = 0;
  let failed = 0;
  let truncated = 0;
  let pending = 0;
  for (const row of rows) {
    if (row.kind === "note") continue;
    const name = row.kind === "thinking" ? "思考" : toolLabel(row.name);
    counts.set(name, (counts.get(name) ?? 0) + 1);
    if (row.kind === "thinking") continue;
    if (!row.result) {
      pending += 1;
      continue;
    }
    durationMs += row.result.duration_ms ?? 0;
    if (row.result.is_error) failed += 1;
    if (row.result.truncated) truncated += 1;
  }
  return {
    tools: [...counts]
      .map(([name, count]) => (count > 1 ? `${name} ×${count}` : name))
      .join(" · "),
    durationMs,
    failed,
    truncated,
    pending,
  };
}

/** 折叠行里的一行摘要：取第一行有意义的内容。 */
export function firstLine(text?: string): string | undefined {
  const line = (text ?? "")
    .split("\n")
    .map((value) => value.trim())
    .find(Boolean);
  return line ? line.slice(0, 100) : undefined;
}
