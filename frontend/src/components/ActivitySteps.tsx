import {
  AlertTriangle,
  Bot,
  ChevronRight,
  FilePenLine,
  FileText,
  Globe,
  Layers,
  Lightbulb,
  Loader2,
  type LucideIcon,
  Search,
  Terminal,
  Wrench,
} from "lucide-react";
import { type ReactNode, useEffect, useRef, useState } from "react";
import type { ChatMessage } from "../types";
import {
  diffLines,
  firstLine,
  formatToolDuration,
  type ToolField,
  toolFieldsOfInput,
  toolSummary,
} from "../lib/format";
import { SplitPatchView } from "./DiffView";

/** 展开状态放在 React 外面：流式刷新会重新挂载卡片，状态装在组件里就会把用户刚打开的那张卡片合上。 */
const expandedCards = new Set<string>();

/** 卡片的图标家族。形状和颜色一起用来分类，只有颜色就成了猜谜。 */
type ToolFamily =
  | "thinking"
  | "shell"
  | "file"
  | "edit"
  | "search"
  | "web"
  | "context"
  | "agent"
  | "tool";

const FAMILY_ICONS: Record<ToolFamily, LucideIcon> = {
  thinking: Lightbulb,
  shell: Terminal,
  file: FileText,
  edit: FilePenLine,
  search: Search,
  web: Globe,
  context: Layers,
  agent: Bot,
  tool: Wrench,
};

const FAMILY_PREFIXES: [string, ToolFamily][] = [
  ["bash", "shell"],
  ["shell", "shell"],
  ["read", "file"],
  ["edit", "edit"],
  ["write", "edit"],
  ["patch", "edit"],
  ["grep", "search"],
  ["glob", "search"],
  ["search_context", "context"],
  ["search", "search"],
  ["web", "web"],
  ["fetch", "web"],
  ["compress", "context"],
  ["decompress", "context"],
  ["memory", "context"],
  ["scratchpad", "context"],
  ["subagent", "agent"],
  ["task", "agent"],
  ["todo", "agent"],
];

function familyOf(name?: string): ToolFamily {
  const key = (name || "").toLowerCase();
  for (const [prefix, family] of FAMILY_PREFIXES) {
    if (key.startsWith(prefix)) return family;
  }
  return "tool";
}

/** Unified-diff fragment Pi attaches to `edit` results. */
function DiffFragment({ diff }: { diff: string }) {
  return (
    <div className="diff-fragment">
      {diffLines(diff).map((line, index) => (
        <div key={index} className={`diff-${line.kind}`}>
          {line.text || "\u00a0"}
        </div>
      ))}
    </div>
  );
}

function argumentsText(args: unknown): string | undefined {
  if (typeof args === "string") return args;
  if (args === undefined || args === null) return undefined;
  try {
    return JSON.stringify(args, null, 2);
  } catch {
    return String(args);
  }
}

/** 参数是一排标量时排成两列表；一旦有嵌套对象就退回 JSON，免得表格里塞一坨未知结构。 */
function ToolArgumentTable({ fields }: { fields: ToolField[] }) {
  return (
    <table className="step-args-table">
      <tbody>
        {fields.map((field) => (
          <tr key={field.key}>
            <td className="step-arg-key">{field.key}</td>
            <td className="step-arg-value">
              {field.multiline ? (
                <pre className="step-arg-block">{field.value}</pre>
              ) : (
                field.value
              )}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function imageParts(result?: ChatMessage) {
  return (result?.parts ?? []).filter(
    (part) => part.type === "image" && part.image_index !== undefined,
  );
}

/** 工具结果的正文：分栏 diff / 行式 diff / 文本 / 图片 / 「无输出」。 */
function ResultBody({
  result,
  sessionId,
}: {
  result?: ChatMessage;
  sessionId?: string;
}) {
  if (!result) return null;
  const texts = (result.parts ?? []).filter(
    (part) => part.type === "text" && part.text?.trim(),
  );
  const images = imageParts(result);
  const empty =
    !result.parts?.length &&
    !result.diff &&
    !result.patch &&
    !result.content?.trim();
  return (
    <>
      {/* 有统一 diff 就画分栏视图，只有带行号的预览才退回原来的行式渲染。 */}
      {result.patch ? (
        <SplitPatchView text={result.patch} />
      ) : (
        result.diff && <DiffFragment diff={result.diff} />
      )}
      {texts.map((part, index) => (
        <pre key={index} className="step-result">
          {part.text}
        </pre>
      ))}
      {sessionId &&
        images.map((part) => (
          <img
            key={`image-${part.image_index}`}
            className="step-image"
            src={`/api/sessions/${encodeURIComponent(sessionId)}/image/${encodeURIComponent(result.id)}/${part.image_index}`}
            alt="工具结果图片"
            loading="lazy"
          />
        ))}
      {empty && <pre className="step-empty">(无输出)</pre>}
    </>
  );
}

/** 一张内联卡片：折叠时只有一行（图标 + 名称 + 摘要 + 耗时），展开挂上正文。 */
function Card({
  cardKey,
  kind,
  name,
  summary,
  failed,
  pending,
  truncated,
  images,
  duration,
  defaultOpen,
  expandable,
  children,
}: {
  cardKey: string;
  kind: ToolFamily;
  name: string;
  summary?: string;
  failed?: boolean;
  pending?: boolean;
  truncated?: boolean;
  images?: number;
  duration?: string;
  defaultOpen?: boolean;
  expandable: boolean;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(
    () => expandedCards.has(cardKey) || !!defaultOpen,
  );
  const firstRun = useRef(true);
  // 全局「默认展开思考」偏好改动时，同步已经挂载的卡片（pi-web 同做法）。
  useEffect(() => {
    if (firstRun.current) {
      firstRun.current = false;
      return;
    }
    setOpen(!!defaultOpen);
  }, [defaultOpen]);
  const Icon = FAMILY_ICONS[kind];
  return (
    <div
      className={`step${failed ? " failed" : ""}${open ? " open" : ""}`}
      data-kind={kind}
    >
      <button
        type="button"
        className="step-row"
        onClick={() => {
          if (!expandable) return;
          if (open) expandedCards.delete(cardKey);
          else expandedCards.add(cardKey);
          setOpen(!open);
        }}
        aria-expanded={expandable ? open : undefined}
        disabled={!expandable}
        title={summary || name}
      >
        <span className="step-icon">
          {failed ? (
            <AlertTriangle size={13} />
          ) : (
            <Icon
              size={13}
              className={
                kind === "thinking"
                  ? `step-bulb${open ? " active" : ""}`
                  : undefined
              }
            />
          )}
        </span>
        <span className="step-name">{name}</span>
        <span className="step-summary">{summary}</span>
        {truncated && <span className="step-badge">已截断</span>}
        {!!images && <span className="step-badge">{images} 张图</span>}
        {pending ? (
          <span className="step-state">
            <Loader2 size={11} className="spin" /> 运行中
          </span>
        ) : (
          duration && <span className="step-time">{duration}</span>
        )}
        {expandable && (
          <ChevronRight
            size={11}
            className={`step-chevron${open ? " open" : ""}`}
          />
        )}
      </button>
      {open && <div className="step-body">{children}</div>}
    </div>
  );
}

/** 思考块：折叠时只剩一行预览，展开看全文；耗时是这一轮回答的生成时间，没有就不显示。 */
export function ThinkingCard({
  id,
  text,
  seconds,
  expanded,
}: {
  id: string;
  text: string;
  seconds?: number;
  expanded?: boolean;
}) {
  return (
    <Card
      cardKey={id}
      kind="thinking"
      name="思考"
      summary={firstLine(text)}
      duration={seconds ? `${seconds}s` : undefined}
      defaultOpen={expanded}
      expandable
    >
      <pre className="step-thinking-text">{text}</pre>
    </Card>
  );
}

/** 工具调用：折叠时一行（原名 + 参数摘要 + 耗时），展开看参数、diff、输出与图片。
 *  名称保持英文原名：中文标签要靠一张映射表，漏一个就显示成错的动词。 */
export function ToolCard({
  id,
  name,
  args,
  result,
  sessionId,
}: {
  id: string;
  name?: string;
  args?: unknown;
  result?: ChatMessage;
  sessionId?: string;
}) {
  const failed = !!result?.is_error;
  // 出错时摘要显示错误本身，比重复参数有用。
  const summary = failed
    ? firstLine(result?.content) || toolSummary(args)
    : toolSummary(args);
  const fields = toolFieldsOfInput(args);
  const argsText = argumentsText(args);
  return (
    <Card
      cardKey={id}
      kind={familyOf(name)}
      name={name || "tool"}
      summary={summary}
      failed={failed}
      pending={!result}
      truncated={!!result?.truncated}
      images={imageParts(result).length}
      duration={formatToolDuration(result?.duration_ms)}
      expandable={!!argsText || !!result}
    >
      {fields ? (
        <ToolArgumentTable fields={fields} />
      ) : (
        argsText && <pre className="step-args">{argsText}</pre>
      )}
      <ResultBody result={result} sessionId={sessionId} />
    </Card>
  );
}
