import { Download } from "lucide-react";
import type { ActivityItem, ChatMessage, Session } from "../types";

type Props = {
  session?: Session;
  messages: ChatMessage[];
  activity: ActivityItem[];
};

function markdownForExport(
  session: Session,
  messages: ChatMessage[],
  activity: ActivityItem[],
): string {
  const lines = [
    `# ${session.title || "Agent Web 会话"}`,
    "",
    `- 会话 ID：${session.id}`,
    `- Agent：${session.agent_id}`,
    `- 工作区：${session.workspace}`,
    `- 模型：${session.model || "默认"}`,
    "",
    "## 对话",
    "",
  ];

  for (const message of messages) {
    const role =
      message.role === "user"
        ? "用户"
        : message.role === "assistant"
          ? "Agent"
          : message.role;
    lines.push(`### ${role}`, "", message.content, "");
  }

  if (activity.length) {
    lines.push("## 执行记录", "");
    for (const item of activity) {
      lines.push(`- **${item.label}**${item.detail ? `：${item.detail}` : ""}`);
    }
    lines.push("");
  }

  return lines.join("\n");
}

export function SessionExportButton({ session, messages, activity }: Props) {
  const exportSession = () => {
    if (!session) return;
    const content = markdownForExport(session, messages, activity);
    const blob = new Blob([content], { type: "text/markdown;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = `${(session.title || "agentweb-session")
      .replace(/[\\/:*?"<>|\u0000-\u001f]/g, "-")
      .trim()
      .slice(0, 100) || "agentweb-session"}.md`;
    anchor.click();
    window.setTimeout(() => URL.revokeObjectURL(url), 1000);
  };

  return (
    <button
      type="button"
      className="message-action"
      onClick={exportSession}
      disabled={!session || messages.length === 0}
      title="导出当前会话为 Markdown"
      aria-label="导出当前会话"
    >
      <Download size={14} />
      <span>导出</span>
    </button>
  );
}
