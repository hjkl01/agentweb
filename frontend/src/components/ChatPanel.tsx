import {
  Activity,
  AlertTriangle,
  AtSign,
  Bot,
  Check,
  Copy,
  GitBranch,
  LoaderCircle,
  Paperclip,
  Quote,
  Send,
  SlidersHorizontal,
  Square,
  User,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { RefObject } from "react";
import type {
  ActivityItem,
  Agent,
  AgentModel,
  ChatMessage,
  Session,
  UploadedFile,
} from "../types";
import { api } from "../lib/api";
import {
  formatDuration,
  responseSeconds,
  toolResultsByCall,
} from "../lib/format";
import type { Preferences } from "../lib/preferences";
import { AgentActivity } from "./AgentActivity";
import { MentionPicker } from "./MentionPicker";
import { ModelSelector } from "./ModelSelector";
import { ThinkingSelector } from "./ThinkingSelector";
import { ToolPresetSelector } from "./ToolPresetSelector";
import { AgentSelector } from "./AgentSelector";
import { MarkdownMessage } from "./MarkdownMessage";
import {
  AssistantParts,
  CompactionMessage,
  ToolResultCard,
} from "./MessageParts";
import { SessionExportButton } from "./SessionExportButton";
import { SessionStats } from "./SessionStats";

/** How many messages the transcript renders before "show earlier". */
const MESSAGE_PAGE = 80;

/** Model and elapsed time of one assistant turn. */
function MessageMeta({ message }: { message: ChatMessage }) {
  const duration = formatDuration(message.duration_ms);
  if (!message.model && !duration) return null;
  return (
    <span className="message-meta">
      {message.model}
      {message.model && duration ? " · " : ""}
      {duration}
    </span>
  );
}

function CopyMessageButton({ content }: { content: string }) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(content);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1600);
    } catch {
      setCopied(false);
    }
  };
  return (
    <button
      type="button"
      className="message-action"
      onClick={copy}
      title="复制消息"
      aria-label="复制消息"
    >
      {copied ? <Check size={13} /> : <Copy size={13} />}
      <span>{copied ? "已复制" : "复制"}</span>
    </button>
  );
}

type Props = {
  current?: Session;
  currentAgent?: Agent;
  agents: Agent[];
  active?: string;
  models: AgentModel[];
  modelLoading: boolean;
  messages: ChatMessage[];
  stream: string;
  activity: ActivityItem[];
  activityOpen: boolean;
  input: string;
  queuedCount: number;
  pendingUser: string[];
  preferences: Preferences;
  textareaRef: RefObject<HTMLTextAreaElement | null>;
  chatRef: RefObject<HTMLElement | null>;
  onInputChange: (value: string) => void;
  onAgentChange: (id: string) => void;
  onModelChange: (model?: string) => void;
  onThinkingChange: (level?: string) => void;
  onToolsChange: (preset?: string) => void;
  onSend: () => void;
  onToggleActivity: () => void;
  onStop: () => void;
  onRestartMissingSession: () => void;
  onForkSession: () => void;
  onForkFromMessage: (entryId: string) => void;
  onNewChat: () => void;
  onOpenSettings: () => void;
  onAutoResize: () => void;
};
export function ChatPanel({
  current,
  currentAgent,
  agents,
  active,
  models,
  modelLoading,
  messages,
  stream,
  activity,
  activityOpen,
  input,
  queuedCount,
  pendingUser,
  preferences,
  textareaRef,
  chatRef,
  onInputChange,
  onAgentChange,
  onModelChange,
  onThinkingChange,
  onToolsChange,
  onSend,
  onToggleActivity,
  onStop,
  onRestartMissingSession,
  onForkSession,
  onForkFromMessage,
  onNewChat,
  onOpenSettings,
  onAutoResize,
}: Props) {
  const running = current?.status === "running";
  // Pi can branch at a user message only while the session is idle and present on disk.
  const canFork =
    currentAgent?.kind === "pi" &&
    !!current?.native_session_id &&
    !running &&
    current?.status !== "missing";
  const [mentionOpen, setMentionOpen] = useState(false);
  const [uploading, setUploading] = useState(false);
  const [dragActive, setDragActive] = useState(false);
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const dragDepth = useRef(0);
  const mentionQuery = useMemo(() => {
    const match = input.match(/(?:^|\s)@(?:\{([^}]*)\}|([^\s]*))$/);
    return match ? (match[1] ?? match[2] ?? "") : "";
  }, [input]);
  const shouldMention = !!active && /(?:^|\s)@(?:\{[^}]*|[^\s]*)$/.test(input);
  // 选中转录文字后，可以在输入框里引用它，不用手动复制粘贴。
  const [quote, setQuote] = useState<string>();
  // 上千条消息的会话一次性布局会卡住主线程，按页向上展开。
  const [messageWindow, setMessageWindow] = useState(MESSAGE_PAGE);
  useEffect(() => {
    setMessageWindow(MESSAGE_PAGE);
  }, [current?.id]);
  const visibleMessages =
    messages.length > messageWindow
      ? messages.slice(messages.length - messageWindow)
      : messages;
  const hiddenCount = messages.length - visibleMessages.length;
  // 工具结果挂回对应的调用卡片；思考耗时用相邻消息的时间差近似。
  // visibleMessages 每次渲染都是新数组，memo 不划算，直接算。
  const toolResults = toolResultsByCall(visibleMessages);
  const thinkingSeconds = responseSeconds(visibleMessages);
  useEffect(() => {
    if (!preferences.quoteSelection) {
      setQuote(undefined);
      return;
    }
    const onSelectionChange = () => {
      const selection = window.getSelection();
      const anchor = selection?.anchorNode ?? null;
      const text =
        selection && !selection.isCollapsed ? selection.toString().trim() : "";
      const node = anchor instanceof Element ? anchor : anchor?.parentElement;
      setQuote(
        text &&
          node &&
          !node.closest("textarea, input") &&
          chatRef.current?.contains(node)
          ? text
          : undefined,
      );
    };
    document.addEventListener("selectionchange", onSelectionChange);
    return () =>
      document.removeEventListener("selectionchange", onSelectionChange);
  }, [chatRef, preferences.quoteSelection]);
  const quoteSelection = () => {
    if (!quote) return;
    onInputChange(
      `${quote
        .split("\n")
        .map((line) => `> ${line}`)
        .join("\n")}\n\n${input}`,
    );
    window.getSelection()?.removeAllRanges();
    setQuote(undefined);
    requestAnimationFrame(() => {
      textareaRef.current?.focus();
      onAutoResize();
    });
  };
  const chooseMention = (path: string) => {
    const match = input.match(/(?:^|\s)@(?:\{[^}]*)?[^\s]*$/);
    if (!match) return;
    const prefix = input.slice(
      0,
      match.index! + (match[0].startsWith(" ") ? 1 : 0),
    );
    const mention = path.includes(" ") ? `@{${path}}` : `@${path}`;
    onInputChange(`${prefix}${mention} `);
    setMentionOpen(false);
    requestAnimationFrame(() => textareaRef.current?.focus());
  };
  const appendMention = (path: string) => {
    const token = path.includes(" ") ? `@{${path}}` : `@${path}`;
    onInputChange(`${input.replace(/\s*$/, " ")}${token} `);
    requestAnimationFrame(() => textareaRef.current?.focus());
  };
  const uploadFiles = async (files: FileList | File[]) => {
    const list = Array.from(files);
    if (!active || list.length === 0) return;
    setUploading(true);
    try {
      for (const file of list) {
        const uploaded = await api<UploadedFile>(
          `/sessions/${active}/upload?name=${encodeURIComponent(file.name)}`,
          {
            method: "POST",
            headers: {
              "Content-Type": file.type || "application/octet-stream",
            },
            body: file,
          },
        );
        appendMention(uploaded.path);
      }
    } catch (error) {
      window.alert(error instanceof Error ? error.message : "文件上传失败");
    } finally {
      setUploading(false);
    }
  };
  return (
    <main
      className="main-panel"
      onDragEnter={(event) => {
        if (!active || !event.dataTransfer.types.includes("Files")) return;
        dragDepth.current += 1;
        setDragActive(true);
      }}
      onDragOver={(event) => {
        if (active && event.dataTransfer.types.includes("Files")) {
          event.preventDefault();
        }
      }}
      onDragLeave={() => {
        dragDepth.current -= 1;
        if (dragDepth.current <= 0) setDragActive(false);
      }}
      onDrop={(event) => {
        dragDepth.current = 0;
        setDragActive(false);
        if (!active) return;
        event.preventDefault();
        void uploadFiles(event.dataTransfer.files);
      }}
    >
      {dragActive && (
        <div className="drop-overlay">
          <Paperclip size={22} />
          <span>松开以上传到工作目录并插入 @ 引用</span>
        </div>
      )}
      <header className="topbar">
        <div className="chat-title">
          <strong>{current?.title || "Agent Web"}</strong>
          {currentAgent && (
            <span>
              <span className="status-dot" /> {currentAgent.name}
            </span>
          )}
        </div>
        {active && (
          <div className="top-actions">
            {/* 窄屏下侧栏整个隐藏，设置只能从这里进。 */}
            <button
              type="button"
              className="mobile-settings"
              onClick={onOpenSettings}
              title="设置"
              aria-label="设置"
            >
              <SlidersHorizontal size={16} />
            </button>
            <SessionStats messages={messages} />
            <AgentSelector
              value={current?.agent_id}
              agents={agents}
              disabled={running}
              onChange={onAgentChange}
            />
            <ModelSelector
              value={current?.model}
              models={models}
              loading={modelLoading}
              disabled={running}
              onChange={onModelChange}
            />
            {currentAgent?.kind === "pi" && (
              <ThinkingSelector
                value={current?.thinking_level}
                disabled={running}
                onChange={onThinkingChange}
              />
            )}
            {currentAgent?.kind === "pi" && (
              <ToolPresetSelector
                value={current?.tool_preset || undefined}
                disabled={running}
                onChange={onToolsChange}
              />
            )}
            <SessionExportButton
              session={current}
              messages={messages}
              activity={activity}
            />
            {canFork && (
              <button
                type="button"
                className="message-action"
                onClick={onForkSession}
                title="从当前 Pi 会话创建独立分支"
              >
                <GitBranch size={14} /> <span>创建分支</span>
              </button>
            )}
          </div>
        )}
      </header>
      <section className="chat" ref={chatRef}>
        {!active && (
          <div className="welcome">
            <div className="welcome-icon">
              <Bot size={30} />
            </div>
            <h1>Build with your Agent</h1>
            <p>
              新建对话会自动使用默认 Agent 和模型。也可以在对话顶部临时切换。
            </p>
            <button className="new-chat" onClick={onNewChat}>
              <span>＋</span> New chat
            </button>
          </div>
        )}
        {current?.status === "missing" && (
          <div className="session-warning" role="alert">
            <AlertTriangle size={18} />
            <div className="session-warning-content">
              <strong>原生会话已不存在</strong>
              <p>
                Agent 的原生 Session
                可能已被手动删除，原有上下文无法恢复。会话元数据已保留；重新开始会创建新的原生
                Session。
              </p>
              <button
                type="button"
                onClick={() => {
                  if (
                    window.confirm(
                      "确认重新开始？旧的原生 Session ID 将被清除，并在发送下一条消息时创建新会话。",
                    )
                  ) {
                    onRestartMissingSession();
                  }
                }}
              >
                重新开始
              </button>
            </div>
          </div>
        )}
        {active && !messages.length && !stream && (
          <div className="welcome compact">
            <div className="welcome-icon">
              <Activity size={24} />
            </div>
            <h2>How can I help?</h2>
            <p>
              向 {currentAgent?.name || current?.agent_id} 描述你想完成的任务。
            </p>
          </div>
        )}
        {active && (running || !messages.length) && (
          <AgentActivity
            items={activity}
            open={activityOpen}
            onToggle={onToggleActivity}
          />
        )}
        {messages.length > 0 && <div className="chat-content-divider" />}
        {hiddenCount > 0 && (
          <button
            type="button"
            className="load-earlier"
            onClick={() => setMessageWindow((value) => value + MESSAGE_PAGE)}
          >
            显示更早的 {Math.min(MESSAGE_PAGE, hiddenCount)} 条（共{" "}
            {messages.length} 条）
          </button>
        )}
        {visibleMessages.map((message) => {
          // 结果已经挂在对应调用卡片上的就跳过；配不到调用的（调用被分页截断）单独成卡片。
          if (message.role === "toolResult")
            return toolResults.get(message.tool_call_id ?? "") ===
              message ? null : (
              <div key={message.id} className="orphan-result">
                <ToolResultCard message={message} sessionId={current?.id} />
              </div>
            );
          if (message.role === "compaction") {
            return (
              <CompactionMessage key={message.id} content={message.content} />
            );
          }
          return (
            <div
              key={message.id}
              className={`message-row ${message.role}${message.stop_reason === "error" ? " failed" : ""}`}
            >
              <div className="avatar">
                {message.role === "user" ? (
                  "U"
                ) : message.role === "error" ? (
                  "!"
                ) : (
                  <Bot size={15} />
                )}
              </div>
              <div className="message-content">
                <div className="message-heading">
                  <div className="message-role">
                    {message.role === "user"
                      ? "You"
                      : message.role === "error"
                        ? "Error"
                        : currentAgent?.name || "Agent"}
                  </div>
                  <MessageMeta message={message} />
                  <CopyMessageButton content={message.content} />
                  {message.role === "user" && canFork && (
                    <button
                      type="button"
                      className="message-action"
                      onClick={() => {
                        if (
                          window.confirm(
                            "从此消息创建分支？当前会话不受影响，这条消息会放回输入框等待修改后重发。",
                          )
                        ) {
                          onForkFromMessage(message.id);
                        }
                      }}
                      title="从这条消息创建独立分支"
                    >
                      <GitBranch size={13} /> <span>从此分叉</span>
                    </button>
                  )}
                </div>
                {message.role === "assistant" ? (
                  <AssistantParts
                    message={message}
                    sessionId={current?.id}
                    toolResults={toolResults}
                    thinkingSeconds={thinkingSeconds.get(message.id)}
                    thinkingExpanded={preferences.thinkingExpanded}
                  />
                ) : (
                  <MarkdownMessage content={message.content} />
                )}
              </div>
            </div>
          );
        })}
        {stream && (
          <div className="message-row assistant">
            <div className="avatar">
              <Bot size={15} />
            </div>
            <div className="message-content">
              <div className="message-role">
                {currentAgent?.name || "Agent"}{" "}
                <span className="streaming">streaming</span>
              </div>
              <MarkdownMessage content={stream} />
              <span className="cursor" />
            </div>
          </div>
        )}
        {pendingUser.map((text) => (
          <div className="message-row user pending" key={text}>
            <div className="avatar">
              <User size={15} />
            </div>
            <div className="message-content">
              <div className="message-role">
                <span className="pending-hint">发送中…</span> You
              </div>
              <MarkdownMessage content={text} />
            </div>
          </div>
        ))}
      </section>
      <div className="composer-wrap">
        <div className="composer-container">
          {mentionOpen && (
            <MentionPicker
              open
              query={mentionQuery}
              sessionId={active!}
              onSelect={chooseMention}
              onClose={() => setMentionOpen(false)}
            />
          )}
          <div className="composer">
            <input
              ref={fileInputRef}
              type="file"
              multiple
              hidden
              onChange={(event) => {
                void uploadFiles(event.target.files ?? []);
                event.target.value = "";
              }}
            />
            {quote && (
              <button
                type="button"
                className="attach-button"
                onClick={quoteSelection}
                title={`引用选中的内容：${quote.slice(0, 80)}`}
                aria-label="引用选区"
              >
                <Quote size={16} />
              </button>
            )}
            <button
              type="button"
              className="attach-button"
              onClick={() => fileInputRef.current?.click()}
              disabled={!active || uploading}
              title="上传文件到工作目录"
              aria-label="上传文件"
            >
              {uploading ? (
                <LoaderCircle size={16} className="spin" />
              ) : (
                <Paperclip size={16} />
              )}
            </button>
            <textarea
              ref={textareaRef}
              value={input}
              onChange={(event) => {
                const value = event.target.value;
                onInputChange(value);
                setMentionOpen(/(?:^|\s)@(?:\{[^}]*|[^\s]*)$/.test(value));
                onAutoResize();
              }}
              onKeyDown={(event) => {
                if (event.key === "Escape" && mentionOpen) {
                  event.preventDefault();
                  setMentionOpen(false);
                  return;
                }
                if (
                  event.key === "Enter" &&
                  !event.shiftKey &&
                  !shouldMention &&
                  (preferences.sendMode !== "ctrlEnter" ||
                    event.ctrlKey ||
                    event.metaKey)
                ) {
                  event.preventDefault();
                  onSend();
                }
              }}
              placeholder={
                active
                  ? "输入消息，使用 @ 选择文件或文件夹…"
                  : "Create a chat to start"
              }
              disabled={!active || current?.status === "missing"}
              rows={1}
            />
            <button
              className="send-button"
              onClick={running ? onStop : onSend}
              disabled={
                !active ||
                current?.status === "missing" ||
                (!running && !input.trim())
              }
              aria-label={running ? "Stop" : "Send"}
              title={running ? "Stop agent" : "Send message"}
            >
              {running ? (
                <Square size={14} strokeWidth={2.2} />
              ) : (
                <Send size={16} />
              )}
            </button>
          </div>
        </div>
        <div className="composer-hint">
          <AtSign size={12} /> @ 选择文件/文件夹 ·{" "}
          {preferences.sendMode === "ctrlEnter"
            ? "Ctrl / Cmd + Enter 发送 · Enter 换行"
            : "Enter 发送 · Shift + Enter 换行"}
          {queuedCount > 0 &&
            ` · 已排队 ${queuedCount} 条，当前回复结束后自动发送`}
        </div>
      </div>
    </main>
  );
}
