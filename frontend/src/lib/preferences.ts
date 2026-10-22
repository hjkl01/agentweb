/**
 * 浏览器本地偏好。只做解析与校验，读写落在 hook 里，这样这段逻辑不碰浏览器全局，
 * 可以在 node 里断言。
 *
 * 深色主题不做：需要把现有一千多处写死的浅色取值整体换成设计变量。
 */
export type SendMode = "enter" | "ctrlEnter";

export type Preferences = {
  completionSound: boolean;
  /** 聊天正文字号（px），只作用于对话与执行过程。 */
  chatFontSize: number;
  /** 聊天内容列宽（px），消息、执行过程、输入框共用。 */
  chatContentWidth: number;
  /** 工具循环里的「思考过程」默认展开。 */
  thinkingExpanded: boolean;
  /** 选中对话文字后显示「引用选区」按钮。 */
  quoteSelection: boolean;
  /** Enter 发送，或者 Ctrl / Cmd + Enter 发送。 */
  sendMode: SendMode;
};

export const CHAT_FONT_SIZE = { min: 12, max: 20, default: 13 };
export const CHAT_CONTENT_WIDTH = { min: 640, max: 1600, default: 900 };

export const DEFAULT_PREFERENCES: Preferences = {
  completionSound: false,
  chatFontSize: CHAT_FONT_SIZE.default,
  chatContentWidth: CHAT_CONTENT_WIDTH.default,
  thinkingExpanded: false,
  quoteSelection: true,
  sendMode: "enter",
};

function clamp(value: unknown, min: number, max: number, fallback: number) {
  if (typeof value !== "number" || !Number.isFinite(value)) return fallback;
  return Math.max(min, Math.min(max, Math.round(value)));
}

export function clampChatFontSize(value: unknown): number {
  return clamp(
    value,
    CHAT_FONT_SIZE.min,
    CHAT_FONT_SIZE.max,
    CHAT_FONT_SIZE.default,
  );
}

export function clampChatContentWidth(value: unknown): number {
  return clamp(
    value,
    CHAT_CONTENT_WIDTH.min,
    CHAT_CONTENT_WIDTH.max,
    CHAT_CONTENT_WIDTH.default,
  );
}

/** 存档可能来自旧版本或被手改过：不认识的值一律退回默认。 */
export function parsePreferences(raw: string | null): Preferences {
  if (!raw) return { ...DEFAULT_PREFERENCES };
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return { ...DEFAULT_PREFERENCES };
  }
  if (typeof parsed !== "object" || parsed === null)
    return { ...DEFAULT_PREFERENCES };
  const stored = parsed as Partial<Preferences>;
  return {
    completionSound: stored.completionSound === true,
    chatFontSize: clampChatFontSize(stored.chatFontSize),
    chatContentWidth: clampChatContentWidth(stored.chatContentWidth),
    thinkingExpanded: stored.thinkingExpanded === true,
    // 引用选区是已有功能，旧存档没有这个键时保持开启。
    quoteSelection: stored.quoteSelection !== false,
    sendMode: stored.sendMode === "ctrlEnter" ? "ctrlEnter" : "enter",
  };
}
