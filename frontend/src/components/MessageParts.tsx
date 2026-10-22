import { AlertTriangle, Brain } from "lucide-react";
import { useState } from "react";
import type { ChatMessage } from "../types";
import { MarkdownMessage } from "./MarkdownMessage";

/** Assistant turns without tool calls can still carry reasoning (and Pi always
 *  reports the final answer's thinking). Tool calls live in `ActivitySteps`. */
function ThinkingPart({
  text,
  defaultOpen,
}: {
  text: string;
  defaultOpen: boolean;
}) {
  // details 的展开状态归浏览器管，这里只同步回来，避免 React 重新渲染时把用户的手动折叠盖掉。
  const [open, setOpen] = useState(defaultOpen);
  return (
    <details
      className="msg-thinking"
      open={open}
      onToggle={(event) => setOpen(event.currentTarget.open)}
    >
      <summary>
        <Brain size={12} /> 思考过程
      </summary>
      <pre>{text}</pre>
    </details>
  );
}

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

/** Body of an assistant message: structured parts when present, else text. */
export function AssistantParts({
  message,
  thinkingExpanded,
}: {
  message: ChatMessage;
  thinkingExpanded?: boolean;
}) {
  const notice = turnNotice(message);
  if (!message.parts?.length)
    return (
      <div className="msg-parts">
        {notice}
        {message.content && <MarkdownMessage content={message.content} />}
      </div>
    );
  return (
    <div className="msg-parts">
      {notice}
      {message.parts.map((part, index) => {
        if (part.type === "thinking")
          return (
            <ThinkingPart
              key={index}
              text={part.text || ""}
              defaultOpen={!!thinkingExpanded}
            />
          );
        if (part.type === "text")
          return <MarkdownMessage key={index} content={part.text || ""} />;
        // Tool calls are rendered by `ActivitySteps`, grouped per turn.
        return null;
      })}
    </div>
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
