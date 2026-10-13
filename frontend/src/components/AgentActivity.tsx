import {
  Activity,
  Brain,
  CheckCircle2,
  ChevronDown,
  Circle,
  FileText,
  Terminal,
  Wrench,
  XCircle,
} from "lucide-react";
import type { ActivityItem } from "../types";

type Props = { items: ActivityItem[]; open: boolean; onToggle: () => void };

function icon(type: string) {
  if (type.startsWith("thinking.")) return <Brain size={14} />;
  if (type.startsWith("tool.")) return <Wrench size={14} />;
  if (type.startsWith("command.")) return <Terminal size={14} />;
  if (type.startsWith("file.")) return <FileText size={14} />;
  if (type === "session.completed") return <CheckCircle2 size={14} />;
  if (type === "agent.error" || type === "error") return <XCircle size={14} />;
  return <Circle size={9} />;
}

function isFailed(type: string) {
  return (
    type === "agent.error" ||
    type === "error" ||
    type.endsWith(".failed") ||
    type.endsWith(".error")
  );
}

function isComplete(type: string) {
  return (
    type === "session.completed" ||
    type.endsWith(".completed") ||
    type.endsWith(".failed") ||
    type.endsWith(".error")
  );
}

function currentStatus(items: ActivityItem[]) {
  const latest = [...items]
    .reverse()
    .find(
      (item) =>
        item.type !== "message.started",
    );
  if (!latest) return { label: "正在处理请求", state: "running" };
  if (latest.type === "session.completed")
    return { label: "已完成", state: "complete" };
  if (latest.type === "message.completed")
    return { label: "本轮输出完成", state: "running" };
  if (isFailed(latest.type)) return { label: "执行出错", state: "error" };
  if (latest.type.startsWith("thinking."))
    return { label: "正在分析", state: "running" };
  if (latest.type.startsWith("tool."))
    return {
      label: isComplete(latest.type) ? "工具调用已返回" : "正在调用工具",
      state: "running",
    };
  if (latest.type.startsWith("command."))
    return {
      label: isComplete(latest.type) ? "命令执行完成" : "正在执行命令",
      state: "running",
    };
  if (latest.type.startsWith("file."))
    return { label: "正在处理文件", state: "running" };
  return { label: latest.label || "正在处理请求", state: "running" };
}

function ActivityRow({ item, expanded }: { item: ActivityItem; expanded: boolean }) {
  const thinking = item.type.startsWith("thinking.");
  const output =
    item.type.startsWith("tool.") || item.type.startsWith("command.");
  const detail = item.detail?.trim();

  return (
    <div
      className={`activity-item ${isFailed(item.type) ? "failed" : ""} ${isComplete(item.type) ? "complete" : ""}`}
      key={item.id}
    >
      <span className="activity-icon">{icon(item.type)}</span>
      <div className="activity-body">
        <div className="activity-label">
          <span>{item.label}</span>
          {isComplete(item.type) && (
            <small>{isFailed(item.type) ? "失败" : "完成"}</small>
          )}
        </div>
        {detail && thinking && (
          <details className="activity-detail activity-thinking" open={expanded}>
            <summary>查看思考记录</summary>
            <pre>{detail}</pre>
          </details>
        )}
        {detail && output && (
          <details className="activity-detail">
            <summary>查看执行详情</summary>
            <pre>{detail}</pre>
          </details>
        )}
        {detail && !thinking && !output && (
          <div className="activity-path" title={detail}>
            {detail}
          </div>
        )}
      </div>
    </div>
  );
}

export function AgentActivity({ items, open, onToggle }: Props) {
  if (!items.length) return null;
  const status = currentStatus(items);

  return (
    <section className={`activity-card ${status.state}`}>
      <button
        className="activity-header"
        onClick={onToggle}
        aria-expanded={open}
      >
        <span className={`activity-status-icon ${status.state}`}>
          {status.state === "complete" ? (
            <CheckCircle2 size={15} />
          ) : status.state === "error" ? (
            <XCircle size={15} />
          ) : (
            <Activity size={15} />
          )}
        </span>
        <span className="activity-heading">
          <strong>{status.label}</strong>
          <small>执行过程 · {items.length} 个步骤</small>
        </span>
        <ChevronDown
          size={15}
          className={`activity-chevron ${open ? "expanded" : ""}`}
        />
      </button>
      {open && (
        <div className="activity-list">
          {items.map((item) => (
            <ActivityRow key={item.id} item={item} expanded={open && status.state === "running" && !items.some((item) => item.type === "session.completed" || item.type === "agent.error" || item.type === "error")} />
          ))}
        </div>
      )}
    </section>
  );
}
