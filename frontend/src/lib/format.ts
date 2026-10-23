/** Small pure formatters shared by the chat transcript. */

import type { ChatMessage } from "../types";

/** Larger JSON results are shown indented as Pi sends them. */
const PRETTY_JSON_MAX_CHARS = 200_000;
const MCP_TOOL_PREFIX = "mcp__";

/** The server and tool an MCP call's result names, or null when there is none. */
function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function mcpToolLabel(
  toolName: string,
  details?: unknown,
): { server: string; tool: string } | null {
  if (!toolName.startsWith(MCP_TOOL_PREFIX) || !isRecord(details)) return null;
  const { server, tool } = details;
  if (
    typeof server !== "string" ||
    typeof tool !== "string" ||
    !server ||
    !tool
  )
    return null;
  return { server, tool };
}

/**
 * MCP servers often answer with one JSON document as text, compacted. Show it
 * indented; anything else, including JSON surrounded by other text, as sent.
 */
export function prettyMcpResultText(text: string): string {
  const trimmed = text.trim();
  if (trimmed.length > PRETTY_JSON_MAX_CHARS || !/^[[{]/.test(trimmed))
    return text;
  try {
    return JSON.stringify(JSON.parse(trimmed), null, 2);
  } catch {
    return text;
  }
}

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

/** 折叠行里的一行摘要：取第一行有意义的内容。 */
export function firstLine(text?: string): string | undefined {
  const line = (text ?? "")
    .split("\n")
    .map((value) => value.trim())
    .find(Boolean);
  return line ? line.slice(0, 100) : undefined;
}

/** 工具结果按 `tool_call_id` 挂回调用卡片。只收窗口里确实出现过那次调用的结果：
 *  调用被分页截断时结果单独成卡片，输出不能被吞掉。 */
export function toolResultsByCall(
  messages: ChatMessage[],
): Map<string, ChatMessage> {
  const called = new Set<string>();
  for (const message of messages) {
    for (const part of message.parts ?? []) {
      if (part.type === "toolCall" && part.tool_call_id) {
        called.add(part.tool_call_id);
      }
    }
  }
  const byCall = new Map<string, ChatMessage>();
  for (const message of messages) {
    if (message.role !== "toolResult" || !message.tool_call_id) continue;
    if (called.has(message.tool_call_id))
      byCall.set(message.tool_call_id, message);
  }
  return byCall;
}

/** 每轮回答的耗时（整秒）：本条助手消息与上一条消息的时间差。
 *  后端只给工具结果写 `duration_ms`，助手消息没有——pi-web 也用相邻消息的时间差近似。 */
export function responseSeconds(messages: ChatMessage[]): Map<string, number> {
  const seconds = new Map<string, number>();
  let previous: number | undefined;
  for (const message of messages) {
    const at = message.created_at ? Date.parse(message.created_at) : NaN;
    if (!Number.isFinite(at)) continue;
    if (message.role === "assistant" && previous !== undefined) {
      const elapsed = Math.round((at - previous) / 1000);
      if (elapsed > 0) seconds.set(message.id, elapsed);
    }
    previous = at;
  }
  return seconds;
}
