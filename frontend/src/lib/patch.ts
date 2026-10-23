/**
 * Unified-diff parsing for the transcript's diff views.
 *
 * Ported from pi-web's `lib/patch.ts` (minus its i18n lookup): a patch becomes
 * rows a view can draw either side by side or as one column of lines.
 */

export type SplitDiffCellType = "context" | "removed" | "added" | "empty";

export interface SplitDiffCell {
  lineNo: number | null;
  text: string;
  type: SplitDiffCellType;
}

export type SplitDiffRow =
  | { type: "hunk"; text: string }
  // 两个 hunk 之间被省略的未改动行。count 为 null 表示补丁里没有行号可算。
  | { type: "gap"; count: number | null }
  | { type: "line"; left: SplitDiffCell; right: SplitDiffCell };

export type SplitDiffGap = Extract<SplitDiffRow, { type: "gap" }>;

export interface SplitDiffFile {
  oldPath?: string;
  newPath?: string;
  rows: SplitDiffRow[];
}

interface PendingChangeLine {
  lineNo: number;
  text: string;
}

export function parseUnifiedPatch(text: string): SplitDiffFile[] | null {
  const files: SplitDiffFile[] = [];
  let current: SplitDiffFile | null = null;
  let pendingOldPath: string | undefined;
  let oldLineNo = 0;
  let newLineNo = 0;
  // 当前 hunk 正文里还剩几行，取自 @@ 头里的计数。任一为正就说明还在 hunk
  // 里：那里以 "--- "/"+++ " 开头的行是内容（比如 "-- x"/"++ x"），不是文件头
  // ——在 hunk 里认文件头会把一个文件拆成一堆假的。
  let hunkOldRemaining = 0;
  let hunkNewRemaining = 0;
  // 当前文件上一个 hunk 结束的位置（同样来自 @@ 头）：下一个 hunk 起点更靠后，
  // 中间那些未改动行就是被省略的。
  let prevHunkEnd: { old: number; new: number } | null = null;
  let removed: PendingChangeLine[] = [];
  let added: PendingChangeLine[] = [];

  const emptyCell = (): SplitDiffCell => ({
    lineNo: null,
    text: "",
    type: "empty",
  });
  const flushChanges = () => {
    if (!current) {
      removed = [];
      added = [];
      return;
    }
    const count = Math.max(removed.length, added.length);
    for (let i = 0; i < count; i++) {
      const left = removed[i]
        ? {
            lineNo: removed[i].lineNo,
            text: removed[i].text,
            type: "removed" as const,
          }
        : emptyCell();
      const right = added[i]
        ? {
            lineNo: added[i].lineNo,
            text: added[i].text,
            type: "added" as const,
          }
        : emptyCell();
      current.rows.push({ type: "line", left, right });
    }
    removed = [];
    added = [];
  };

  for (const line of text.split(/\r?\n/)) {
    const insideHunk = hunkOldRemaining > 0 || hunkNewRemaining > 0;

    // 文件头只出现在 hunk 之间，hunk 正文里不会出现。
    if (!insideHunk) {
      if (line.startsWith("--- ")) {
        flushChanges();
        pendingOldPath = cleanPatchPath(line.slice(4));
        continue;
      }

      if (line.startsWith("+++ ")) {
        flushChanges();
        current = {
          oldPath: pendingOldPath,
          newPath: cleanPatchPath(line.slice(4)),
          rows: [],
        };
        files.push(current);
        prevHunkEnd = null;
        continue;
      }
    }

    const hunk = line.match(/^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/);
    if (hunk) {
      if (!current) {
        current = { rows: [] };
        files.push(current);
      }
      flushChanges();
      oldLineNo = Number(hunk[1]);
      newLineNo = Number(hunk[3]);
      hunkOldRemaining = hunk[2] === undefined ? 1 : Number(hunk[2]);
      hunkNewRemaining = hunk[4] === undefined ? 1 : Number(hunk[4]);
      // 某一边在这个 hunk 里没有行，行号指向它前面那一行（"-5,0"）。
      const oldStart = hunkOldRemaining === 0 ? oldLineNo + 1 : oldLineNo;
      const newStart = hunkNewRemaining === 0 ? newLineNo + 1 : newLineNo;
      if (prevHunkEnd) {
        const oldGap = oldStart - prevHunkEnd.old;
        const newGap = newStart - prevHunkEnd.new;
        if (oldGap > 0 || newGap > 0) {
          current.rows.push({
            type: "gap",
            count: oldGap === newGap ? oldGap : null,
          });
        }
      }
      prevHunkEnd = {
        old: oldStart + hunkOldRemaining,
        new: newStart + hunkNewRemaining,
      };
      current.rows.push({ type: "hunk", text: line });
      continue;
    }

    if (!current) continue;

    if (line.startsWith("\\ ")) {
      flushChanges();
      current.rows.push({ type: "hunk", text: line });
      continue;
    }

    const prefix = line[0];
    const content = line.slice(1);

    if (prefix === " ") {
      flushChanges();
      current.rows.push({
        type: "line",
        left: { lineNo: oldLineNo++, text: content, type: "context" },
        right: { lineNo: newLineNo++, text: content, type: "context" },
      });
      if (hunkOldRemaining > 0) hunkOldRemaining--;
      if (hunkNewRemaining > 0) hunkNewRemaining--;
    } else if (prefix === "-") {
      removed.push({ lineNo: oldLineNo++, text: content });
      if (hunkOldRemaining > 0) hunkOldRemaining--;
    } else if (prefix === "+") {
      added.push({ lineNo: newLineNo++, text: content });
      if (hunkNewRemaining > 0) hunkNewRemaining--;
    } else if (line !== "") {
      flushChanges();
      current.rows.push({ type: "hunk", text: line });
    }
  }

  flushChanges();

  const parsed = files.filter((file) =>
    file.rows.some((row) => row.type === "line"),
  );
  return parsed.length > 0 ? parsed : null;
}

/** 单列 diff 里的一行：新旧两套行号，哪套适用就用哪套。 */
export interface UnifiedDiffLine {
  type: "context" | "removed" | "added";
  text: string;
  oldLineNo: number | null;
  newLineNo: number | null;
}

/**
 * 一个文件的行改成单列。左右并排时成对的那两行变成上下两行（删的在上、增的
 * 在下），上下文行只出现一次而不是重复两份；hunk 头不携带文件内容，和并排视
 * 图一样省掉；hunk 之间的省略行仍然是一行省略。
 */
export function unifiedDiffLines(
  file: SplitDiffFile,
): Array<UnifiedDiffLine | SplitDiffGap> {
  const lines: Array<UnifiedDiffLine | SplitDiffGap> = [];
  for (const row of file.rows) {
    if (row.type === "hunk") continue;
    if (row.type === "gap") {
      lines.push(row);
      continue;
    }
    if (row.left.type === "context" && row.right.type === "context") {
      lines.push({
        type: "context",
        text: row.right.text,
        oldLineNo: row.left.lineNo,
        newLineNo: row.right.lineNo,
      });
      continue;
    }
    if (row.left.type !== "empty") {
      lines.push({
        type: "removed",
        text: row.left.text,
        oldLineNo: row.left.lineNo,
        newLineNo: null,
      });
    }
    if (row.right.type !== "empty") {
      lines.push({
        type: "added",
        text: row.right.text,
        oldLineNo: null,
        newLineNo: row.right.lineNo,
      });
    }
  }
  return lines;
}

/** 省略行条上的文案。 */
export function diffGapLabel(count: number): string {
  return `${count} 行未改动`;
}

function cleanPatchPath(path: string): string {
  return path.split("\t")[0].trim();
}
