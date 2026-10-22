import { useMemo } from "react";
import type { WorkspaceFile } from "../types";

export type PreviewKind = "image" | "audio" | "pdf" | "csv";

const IMAGE_EXT = new Set([
  "png",
  "jpg",
  "jpeg",
  "gif",
  "webp",
  "avif",
  "bmp",
  "ico",
  "svg",
]);
const AUDIO_EXT = new Set(["mp3", "wav", "ogg", "oga", "m4a", "flac"]);

/** Decide how (or whether) a workspace file can be previewed inline. */
export function previewKind(path: string): PreviewKind | undefined {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  if (IMAGE_EXT.has(ext)) return "image";
  if (AUDIO_EXT.has(ext)) return "audio";
  if (ext === "pdf") return "pdf";
  if (ext === "csv" || ext === "tsv") return "csv";
  return undefined;
}

function rawUrl(sessionId: string, path: string): string {
  const encoded = path.split("/").map(encodeURIComponent).join("/");
  return `/api/sessions/${encodeURIComponent(sessionId)}/raw/${encoded}`;
}

const MAX_CSV_ROWS = 500;
const MAX_CSV_COLS = 50;

function parseDelimited(text: string, delimiter: string): string[][] {
  const rows: string[][] = [];
  let row: string[] = [];
  let field = "";
  let quoted = false;
  for (let index = 0; index < text.length; index += 1) {
    const char = text[index];
    if (quoted) {
      if (char === '"') {
        if (text[index + 1] === '"') {
          field += '"';
          index += 1;
        } else {
          quoted = false;
        }
      } else {
        field += char;
      }
    } else if (char === '"') {
      quoted = true;
    } else if (char === delimiter) {
      row.push(field);
      field = "";
    } else if (char === "\n") {
      row.push(field);
      rows.push(row);
      row = [];
      field = "";
    } else if (char !== "\r") {
      field += char;
    }
  }
  if (field.length || row.length) {
    row.push(field);
    rows.push(row);
  }
  return rows;
}

function CsvPreview({ content }: { content: string }) {
  const { rows, truncated } = useMemo(() => {
    const delimiter = content.includes("\t") ? "\t" : ",";
    const parsed = parseDelimited(content, delimiter).filter(
      (row) => row.length > 1 || row[0] !== "",
    );
    return {
      rows: parsed
        .slice(0, MAX_CSV_ROWS)
        .map((row) => row.slice(0, MAX_CSV_COLS)),
      truncated:
        parsed.length > MAX_CSV_ROWS ||
        parsed.some((row) => row.length > MAX_CSV_COLS),
    };
  }, [content]);
  if (!rows.length) return <div className="rich-empty">空表格</div>;
  return (
    <div className="rich-csv">
      <table>
        <tbody>
          {rows.map((row, rowIndex) => (
            <tr key={rowIndex}>
              {row.map((cell, cellIndex) => (
                <td key={cellIndex}>{cell}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
      {truncated && (
        <div className="rich-notice">
          仅显示前 {MAX_CSV_ROWS} 行 / {MAX_CSV_COLS} 列
        </div>
      )}
    </div>
  );
}

export function RichPreview({
  sessionId,
  file,
  kind,
}: {
  sessionId: string;
  file: WorkspaceFile;
  kind: PreviewKind;
}) {
  const src = rawUrl(sessionId, file.path);
  if (kind === "image")
    return <img className="rich-image" src={src} alt={file.path} />;
  if (kind === "audio")
    return <audio className="rich-audio" controls src={src} />;
  if (kind === "pdf")
    return <iframe className="rich-pdf" src={src} title={file.path} />;
  return <CsvPreview content={file.content} />;
}
