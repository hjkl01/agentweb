import {
  Activity,
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
import { useState } from "react";
import {
  activityFacts,
  type ActivityRow,
  diffLines,
  firstLine,
  formatToolDuration,
  type ToolField,
  toolFieldsOfInput,
  toolLabel,
} from "../lib/format";
import { MarkdownMessage } from "./MarkdownMessage";
import { SplitPatchView } from "./DiffView";

/** 展开状态放在 React 外面：流式刷新会重新挂载步骤行，状态装在组件里就会把用户刚打开的那张卡片合上。 */
const expandedRows = new Set<string>();

/** 步骤行的图标家族。形状和颜色一起用来分类，只有颜色就成了猜谜。 */
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

/** Expanded step: what was asked for, then what came back. */
function StepBody({
  row,
  sessionId,
}: {
  row: ActivityRow;
  sessionId?: string;
}) {
  const result = row.result;
  const args = argumentsText(row.args);
  const fields = toolFieldsOfInput(row.args);
  const texts = (result?.parts ?? []).filter(
    (part) => part.type === "text" && part.text?.trim(),
  );
  const images = (result?.parts ?? []).filter(
    (part) => part.type === "image" && part.image_index !== undefined,
  );
  const empty =
    !result?.parts?.length &&
    !result?.diff &&
    !result?.patch &&
    !result?.content?.trim();
  return (
    <div className="step-body">
      {row.kind === "thinking" && row.text && (
        <pre className="step-thinking-text">{row.text}</pre>
      )}
      {fields ? (
        <ToolArgumentTable fields={fields} />
      ) : (
        args && <pre className="step-args">{args}</pre>
      )}
      {/* 有统一 diff 就画分栏视图，只有带行号的预览才退回原来的行式渲染。 */}
      {result?.patch ? (
        <SplitPatchView text={result.patch} />
      ) : (
        result?.diff && <DiffFragment diff={result.diff} />
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
            src={`/api/sessions/${encodeURIComponent(sessionId)}/image/${encodeURIComponent(result!.id)}/${part.image_index}`}
            alt="工具结果图片"
            loading="lazy"
          />
        ))}
      {result && empty && <pre className="step-empty">(无输出)</pre>}
    </div>
  );
}

function StepRow({
  row,
  sessionId,
  thinkingExpanded,
}: {
  row: ActivityRow;
  sessionId?: string;
  thinkingExpanded?: boolean;
}) {
  const thinking = row.kind === "thinking";
  const family = thinking ? "thinking" : familyOf(row.name);
  const [open, setOpen] = useState(
    () => expandedRows.has(row.key) || (thinking && !!thinkingExpanded),
  );
  const result = row.result;
  const failed = !!result?.is_error;
  const pending = row.kind === "tool" && !result;
  const images = (result?.parts ?? []).filter(
    (part) => part.type === "image" && part.image_index !== undefined,
  ).length;
  const expandable = thinking || !!argumentsText(row.args) || !!result;
  const Icon = FAMILY_ICONS[family];
  // 出错时摘要显示错误本身，比重复参数有用。
  const summary = failed
    ? firstLine(result?.content) || row.summary
    : row.summary;
  // 快过 100ms 的工具不显示耗时：每次都写 0.0s 只是噪音。
  const duration = formatToolDuration(result?.duration_ms);
  return (
    <div
      className={`step${failed ? " failed" : ""}${open ? " open" : ""}`}
      data-kind={row.kind === "note" ? "note" : family}
    >
      {row.kind === "note" ? (
        <div className="step-note">
          <MarkdownMessage content={row.text || ""} />
        </div>
      ) : (
        <button
          type="button"
          className="step-row"
          onClick={() => {
            if (!expandable) return;
            if (open) expandedRows.delete(row.key);
            else expandedRows.add(row.key);
            setOpen(!open);
          }}
          aria-expanded={expandable ? open : undefined}
          disabled={!expandable}
          title={summary || row.name || firstLine(row.text)}
        >
          <span className="step-icon">
            {failed ? (
              <AlertTriangle size={13} />
            ) : (
              <Icon
                size={13}
                className={
                  thinking ? `step-bulb${open ? " active" : ""}` : undefined
                }
              />
            )}
          </span>
          <span className="step-name">
            {thinking ? "思考" : toolLabel(row.name)}
          </span>
          <span className="step-summary">
            {thinking ? firstLine(row.text) : summary}
          </span>
          {result?.truncated && <span className="step-badge">已截断</span>}
          {images > 0 && <span className="step-badge">{images} 张图</span>}
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
      )}
      {open && <StepBody row={row} sessionId={sessionId} />}
    </div>
  );
}

/** One turn's thinking + tool calls + results, as a single collapsible block. */
export function ActivityGroup({
  rows,
  sessionId,
  thinkingExpanded,
  defaultOpen,
}: {
  rows: ActivityRow[];
  sessionId?: string;
  thinkingExpanded?: boolean;
  defaultOpen?: boolean;
}) {
  const facts = activityFacts(rows);
  // 默认展开只在「这是最新一段、且回答还没出来」时生效；用户点过就以点击为准。
  const [manual, setManual] = useState<boolean | null>(null);
  const open = manual ?? !!defaultOpen;
  const duration = formatToolDuration(facts.durationMs);
  return (
    <div className={`activity-steps${facts.failed ? " failed" : ""}`}>
      <button
        type="button"
        className="activity-header"
        onClick={() => setManual(!open)}
        aria-expanded={open}
      >
        <span className="activity-icon">
          <Activity size={14} />
        </span>
        <span className="activity-title">执行过程</span>
        <span className="activity-facts">{facts.tools}</span>
        {/* 耗时单独一列：它比工具清单重要，不能被省略号先截掉。 */}
        {duration && <span className="activity-duration">{duration}</span>}
        {facts.failed > 0 && (
          <span className="activity-badge failed">{facts.failed} 步出错</span>
        )}
        {facts.truncated > 0 && (
          <span className="activity-badge">{facts.truncated} 步输出截断</span>
        )}
        {facts.pending > 0 && (
          <span className="activity-live">
            <Loader2 size={11} className="spin" /> 运行中
          </span>
        )}
        <ChevronRight
          size={13}
          className={`activity-chevron${open ? " open" : ""}`}
        />
      </button>
      {open && (
        <div className="step-list">
          {rows.map((row) => (
            <StepRow
              key={row.key}
              row={row}
              sessionId={sessionId}
              thinkingExpanded={thinkingExpanded}
            />
          ))}
        </div>
      )}
    </div>
  );
}
