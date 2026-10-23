import { useEffect, useMemo, useState } from "react";
import {
  diffGapLabel,
  parseUnifiedPatch,
  type SplitDiffCell,
  type SplitDiffFile,
  type SplitDiffGap,
  unifiedDiffLines,
  type UnifiedDiffLine,
} from "../lib/patch";

export type PatchDiffLayout = "split" | "stacked";

/** 窄屏放不下两列代码：改成单列，删的行压在替换它的那一行上面。 */
function useNarrowViewport(): boolean {
  const query = "(max-width: 720px)";
  const [narrow, setNarrow] = useState(
    () => typeof window !== "undefined" && window.matchMedia(query).matches,
  );
  useEffect(() => {
    const media = window.matchMedia(query);
    const onChange = () => setNarrow(media.matches);
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
  }, []);
  return narrow;
}

/**
 * 工具结果里的改动：解析得动就画成分栏 diff，解析不动就退回等宽文本，
 * 别把补丁本身藏起来。
 */
export function SplitPatchView({ text }: { text: string }) {
  const files = useMemo(() => parseUnifiedPatch(text), [text]);
  const narrow = useNarrowViewport();
  if (!files) return <PatchDiffTextView text={text} />;
  return <PatchDiffFiles files={files} layout={narrow ? "stacked" : "split"} />;
}

export function PatchDiffFiles({
  files,
  layout,
}: {
  files: SplitDiffFile[];
  layout: PatchDiffLayout;
}) {
  const many = files.length > 1;
  return (
    <div className={`patch-files${many ? " many" : ""}`}>
      {files.map((file, index) => (
        <div key={index} className="patch-file">
          {many && (
            <div className="patch-file-path">
              {file.newPath || file.oldPath || "补丁"}
            </div>
          )}
          {layout === "split" ? (
            <SplitDiffHeader file={file} />
          ) : (
            <StackedDiffBody file={file} />
          )}
        </div>
      ))}
    </div>
  );
}

function SplitDiffHeader({ file }: { file: SplitDiffFile }) {
  return (
    <div className="patch-scroll">
      {file.rows.map((row, index) => {
        if (row.type === "hunk") {
          return (
            <div key={index} className="patch-hunk">
              {row.text}
            </div>
          );
        }
        if (row.type === "gap") {
          return <DiffGapRow key={index} gap={row} />;
        }
        return (
          <div key={index} className="patch-row">
            <SplitDiffCellView cell={row.left} />
            <SplitDiffCellView cell={row.right} />
          </div>
        );
      })}
    </div>
  );
}

function StackedDiffBody({ file }: { file: SplitDiffFile }) {
  return (
    <div className="patch-scroll">
      {unifiedDiffLines(file).map((line, index) =>
        "type" in line && line.type === "gap" ? (
          <DiffGapRow key={index} gap={line} />
        ) : (
          <StackedDiffLineView key={index} line={line as UnifiedDiffLine} />
        ),
      )}
    </div>
  );
}

function DiffGapRow({ gap }: { gap: SplitDiffGap }) {
  return (
    <div className="patch-gap">
      {gap.count && gap.count > 0 ? `⋯ ${diffGapLabel(gap.count)}` : "⋯"}
    </div>
  );
}

function CellText({
  text,
  lineNo,
  marker,
}: {
  text: string;
  lineNo: number | null;
  marker: string;
}) {
  return (
    <>
      <span className="patch-gutter">{lineNo ?? ""}</span>
      <span className="patch-marker">{marker}</span>
      <span className="patch-text">{text}</span>
    </>
  );
}

function SplitDiffCellView({ cell }: { cell: SplitDiffCell }) {
  const marker =
    cell.type === "added" ? "+" : cell.type === "removed" ? "-" : " ";
  return (
    <div className={`patch-cell ${cell.type}`}>
      {cell.type === "empty" ? null : (
        <CellText text={cell.text} lineNo={cell.lineNo} marker={marker} />
      )}
    </div>
  );
}

function StackedDiffLineView({ line }: { line: UnifiedDiffLine }) {
  const marker =
    line.type === "added" ? "+" : line.type === "removed" ? "-" : " ";
  return (
    <div className={`patch-cell ${line.type}`}>
      <CellText
        text={line.text}
        lineNo={line.newLineNo ?? line.oldLineNo}
        marker={marker}
      />
    </div>
  );
}

function PatchDiffTextView({ text }: { text: string }) {
  return <pre className="patch-plain">{text}</pre>;
}
