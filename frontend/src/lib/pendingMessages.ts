import type { ChatMessage } from "../types";

/**
 * 本地已回显、但 Pi 转录里还没有的用户消息（正在排队或刚发出去）。
 *
 * 转录是消息的唯一真相：服务端整表替换 `messages` 时，如果这条文本已经出现在
 * 转录里，就不再需要本地回显；否则必须留着，否则追加的任务会从页面上消失。
 */
export function settlePending(
  pending: string[],
  messages: ChatMessage[],
): string[] {
  if (!pending.length) return pending;
  const recorded = new Set(
    messages
      .filter((message) => message.role === "user")
      .map((message) => message.content.trim()),
  );
  const next = pending.filter((text) => !recorded.has(text.trim()));
  // 没有变化时返回同一个数组，省掉一次渲染。
  return next.length === pending.length ? pending : next;
}
