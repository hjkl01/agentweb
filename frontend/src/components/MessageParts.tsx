import { AlertTriangle, FileText, GitBranch } from "lucide-react";
import { useState } from "react";
import type { WrittenFile } from "../lib/turnFiles";
import type { BranchOption, ChatMessage } from "../types";
import { ThinkingCard, ToolCard } from "./ActivitySteps";
import { MarkdownMessage } from "./MarkdownMessage";

/** Provider failure or truncation notice for one assistant turn. */
function turnNotice(message: ChatMessage) {
  if (message.stop_reason === "error")
    return (
      <div className="turn-error">
        <AlertTriangle size={13} />
        <span>{message.error_message || "本轮请求失败，未返回内容。"}</span>
      </div>
    );
  if (message.stop_reason === "length")
    return (
      <div className="turn-notice">输出达到长度上限，本轮回答被 Pi 截断。</div>
    );
  return null;
}

/** Body of an assistant message: structured parts when present, else text.
 *  思考和工具调用按转录顺序内联成卡片，工具结果按 `tool_call_id` 挂回对应的调用卡片。 */
export function AssistantParts({
  message,
  sessionId,
  toolResults,
  thinkingSeconds,
  thinkingExpanded,
}: {
  message: ChatMessage;
  sessionId?: string;
  toolResults?: Map<string, ChatMessage>;
  thinkingSeconds?: number;
  thinkingExpanded?: boolean;
}) {
  const notice = turnNotice(message);
  const parts = message.parts ?? [];
  if (!parts.length)
    return (
      <div className="msg-parts">
        {notice}
        {message.content && <MarkdownMessage content={message.content} />}
      </div>
    );
  return (
    <div className="msg-parts">
      {notice}
      {parts.map((part, index) => {
        const key = `${message.id}:${index}`;
        if (part.type === "thinking")
          return (
            <ThinkingCard
              key={key}
              id={key}
              text={part.text || ""}
              seconds={thinkingSeconds}
              expanded={thinkingExpanded}
            />
          );
        if (part.type === "toolCall")
          return (
            <ToolCard
              key={key}
              id={part.tool_call_id || key}
              name={part.name}
              args={part.arguments}
              result={
                part.tool_call_id
                  ? toolResults?.get(part.tool_call_id)
                  : undefined
              }
              sessionId={sessionId}
            />
          );
        if (part.type === "text")
          return <MarkdownMessage key={key} content={part.text || ""} />;
        return null;
      })}
    </div>
  );
}

/** 本轮写入/修改过的文件：点开在工作区面板里预览。 */
export function WrittenFiles({
  files,
  onOpenFile,
}: {
  files: WrittenFile[];
  onOpenFile?: (path: string) => void;
}) {
  return (
    <div className="written-files">
      {files.map((file) => (
        <button
          key={`${file.callId}:${file.path}`}
          type="button"
          className="written-file"
          title={file.path}
          onClick={() => onOpenFile?.(file.path)}
        >
          <FileText size={12} />
          {file.path.split("/").pop()}
        </button>
      ))}
    </div>
  );
}

/**
 * pi 把会话存成树，转录只显示其中一条根→叶路径。这里给出岔口上的其它去向：
 * 当前看的是默认分支时列出别的分支，看的是历史分支时给出回去的按钮。
 */
export function BranchSwitcher({
  options,
  leaf,
  onSelect,
}: {
  options: BranchOption[];
  leaf?: string;
  onSelect: (leaf?: string) => void;
}) {
  const current = leaf ?? options.find((option) => option.live)?.id;
  const others = options.filter((option) => option.id !== current);
  if (!others.length) return null;
  return (
    <div className="branch-switch">
      <GitBranch size={13} />
      {others.map((option) => (
        <button
          key={option.id}
          type="button"
          className="branch-option"
          title={`${option.role || "entry"} · ${option.entries} 条：${option.preview}`}
          onClick={() => onSelect(option.live ? undefined : option.id)}
        >
          {option.live
            ? "返回当前分支"
            : `切换到别的分支 · ${option.preview || option.id}`}
        </button>
      ))}
    </div>
  );
}

/** 配不到调用的工具结果（调用被分页截断）：单独一张卡片，输出不能丢。 */
export function ToolResultCard({
  message,
  sessionId,
}: {
  message: ChatMessage;
  sessionId?: string;
}) {
  return (
    <ToolCard
      id={message.id}
      name={message.tool_name}
      result={message}
      sessionId={sessionId}
    />
  );
}

export function CompactionMessage({ content }: { content: string }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="compaction-message">
      <button
        type="button"
        className="compaction-header"
        onClick={() => setOpen((value) => !value)}
        aria-expanded={open}
      >
        上下文已压缩，点击查看摘要
      </button>
      {open && <pre>{content}</pre>}
    </div>
  );
}
