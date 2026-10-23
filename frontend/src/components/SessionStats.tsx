import { Gauge } from "lucide-react";
import { useMemo } from "react";
import type { ChatMessage, MessageUsage } from "../types";

function formatTokens(value: number): string {
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(1)}k`;
  return String(value);
}

function sumUsage(usage: MessageUsage) {
  return (
    (usage.input || 0) +
    (usage.output || 0) +
    (usage.cacheRead || 0) +
    (usage.cacheWrite || 0)
  );
}

/** Compact token/cost summary aggregated from Pi `message.usage` fields. */
export function SessionStats({
  messages,
  contextWindow,
}: {
  messages: ChatMessage[];
  /** 当前模型的上下文窗口（tokens），未知时不显示占比。 */
  contextWindow?: number;
}) {
  const stats = useMemo(() => {
    let tokens = 0;
    let cost = 0;
    let turns = 0;
    let context = 0;
    for (const message of messages) {
      const usage = message.usage;
      if (!usage) continue;
      tokens += sumUsage(usage);
      cost += usage.cost?.total || 0;
      if (message.role === "assistant") {
        turns += 1;
        // The latest assistant prompt size approximates current context load.
        context =
          (usage.input || 0) + (usage.cacheRead || 0) + (usage.cacheWrite || 0);
      }
    }
    return { tokens, cost, turns, context };
  }, [messages]);

  if (!stats.turns && !stats.tokens) return null;
  // 上下文用量仪表：超出窗口时按 100% 显示，否则取整数百分比。
  const percent = contextWindow
    ? Math.min(100, Math.round((stats.context / contextWindow) * 100))
    : undefined;
  const detail = [
    percent === undefined
      ? `上下文约 ${stats.context.toLocaleString()} tokens`
      : `上下文 ${stats.context.toLocaleString()} / ${contextWindow!.toLocaleString()} tokens（${percent}%）`,
    `累计 ${stats.tokens.toLocaleString()} tokens`,
    `费用 $${stats.cost.toFixed(4)}`,
    `${stats.turns} 轮`,
  ].join("\n");
  return (
    <span
      className={`session-stats${percent !== undefined && percent >= 80 ? " warn" : ""}`}
      title={detail}
    >
      <Gauge size={13} />
      <span>
        {formatTokens(stats.context)} ctx
        {percent === undefined ? "" : ` · ${percent}%`}
      </span>
      <span className="session-stats-cost">${stats.cost.toFixed(3)}</span>
    </span>
  );
}
