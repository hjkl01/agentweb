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
export function SessionStats({ messages }: { messages: ChatMessage[] }) {
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
  const detail = [
    `上下文约 ${stats.context.toLocaleString()} tokens`,
    `累计 ${stats.tokens.toLocaleString()} tokens`,
    `费用 $${stats.cost.toFixed(4)}`,
    `${stats.turns} 轮`,
  ].join("\n");
  return (
    <span className="session-stats" title={detail}>
      <Gauge size={13} />
      <span>{formatTokens(stats.context)} ctx</span>
      <span className="session-stats-cost">${stats.cost.toFixed(3)}</span>
    </span>
  );
}
