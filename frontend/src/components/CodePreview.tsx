import { Copy, FileCode2 } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { CodeTokens } from "./CodeTokens";
import { MarkdownMessage } from "./MarkdownMessage";
import type { WorkspaceFile } from "../types";

type Props = { file: WorkspaceFile; focusLine?: number };

const languageNames: Record<string, string> = {
  rs: "Rust",
  ts: "TypeScript",
  tsx: "TSX",
  js: "JavaScript",
  jsx: "JSX",
  json: "JSON",
  py: "Python",
  go: "Go",
  java: "Java",
  css: "CSS",
  html: "HTML",
  md: "Markdown",
  yaml: "YAML",
  yml: "YAML",
  toml: "TOML",
  sh: "Shell",
  bash: "Shell",
  sql: "SQL",
};

function language(path: string) {
  const extension = path.split(".").pop()?.toLowerCase() || "";
  return languageNames[extension] || extension.toUpperCase() || "Text";
}

export function CodePreview({ file, focusLine }: Props) {
  const [copied, setCopied] = useState(false);
  const lines = useMemo(() => file.content.split("\n"), [file.content]);
  const lang = language(file.path);
  const extension = file.path.split(".").pop()?.toLowerCase() || "";
  const isMarkdown = extension === "md" || extension === "markdown";
  const isHtml = extension === "html" || extension === "htm";
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!focusLine || focusLine < 1) return;
    const line = scrollRef.current?.querySelector<HTMLElement>(
      `[data-line="${focusLine}"]`,
    );
    line?.scrollIntoView({ block: "center", behavior: "smooth" });
  }, [focusLine, file.path]);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(file.content);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1200);
    } catch {
      setCopied(false);
    }
  };

  return (
    <div className="code-preview">
      <div className="code-preview-head">
        <span className="code-preview-language">
          <FileCode2 size={13} />
          {lang}
        </span>
        <span>{lines.length.toLocaleString()} lines</span>
        <button className="code-copy" onClick={copy} title="Copy file">
          <Copy size={13} />
          {copied ? "Copied" : "Copy"}
        </button>
      </div>
      {isMarkdown ? (
        <div className="workspace-markdown-preview">
          <MarkdownMessage content={file.content} />
        </div>
      ) : isHtml ? (
        <iframe
          className="workspace-html-preview"
          title={`HTML preview: ${file.path}`}
          sandbox=""
          referrerPolicy="no-referrer"
          srcDoc={file.content}
        />
      ) : (
        <div className="code-preview-scroll" ref={scrollRef}>
          <pre>
            {lines.map((line, index) => {
              const lineNumber = index + 1;
              return (
                <span
                  className={`code-line${focusLine === lineNumber ? " code-line-focused" : ""}`}
                  data-line={lineNumber}
                  key={index}
                >
                  <span className="code-line-number">{lineNumber}</span>
                  <code>
                    <CodeTokens line={line || " "} />
                  </code>
                </span>
              );
            })}
          </pre>
        </div>
      )}
    </div>
  );
}
