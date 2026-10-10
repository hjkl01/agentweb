import { Check, Copy } from "lucide-react";
import { Fragment, useState } from "react";

type InlinePart = { type: "text" | "code" | "link" | "strong" | "em"; value: string; href?: string };

function inlineParts(source: string): InlinePart[] {
  const parts: InlinePart[] = [];
  const pattern = /`([^`]+)`|\*\*([\s\S]+?)\*\*|__([\s\S]+?)__|\*([^*\n]+)\*|_([^_\n]+)_|\[([^\]]+)\]\(([^\s)]+)(?:\s+["'][^"']*["'])?\)/g;
  let last = 0;
  for (const match of source.matchAll(pattern)) {
    const index = match.index ?? 0;
    if (index > last) parts.push({ type: "text", value: source.slice(last, index) });
    if (match[1] !== undefined) parts.push({ type: "code", value: match[1] });
    else if (match[2] !== undefined || match[3] !== undefined) parts.push({ type: "strong", value: match[2] ?? match[3] });
    else if (match[4] !== undefined || match[5] !== undefined) parts.push({ type: "em", value: match[4] ?? match[5] });
    else {
      const href = match[7];
      const safe = /^(https?:|mailto:|\/|#)/i.test(href) ? href : undefined;
      parts.push(safe
        ? { type: "link", value: match[6], href: safe }
        : { type: "text", value: match[0] });
    }
    last = index + match[0].length;
  }
  if (last < source.length) parts.push({ type: "text", value: source.slice(last) });
  return parts;
}

function Inline({ source }: { source: string }) {
  return <>{inlineParts(source).map((part, index) => {
    if (part.type === "code") return <code className="md-inline-code" key={index}>{part.value}</code>;
    if (part.type === "strong") return <strong key={index}>{part.value}</strong>;
    if (part.type === "em") return <em key={index}>{part.value}</em>;
    if (part.type === "link") return <a key={index} href={part.href} target={part.href?.startsWith("http") ? "_blank" : undefined} rel={part.href?.startsWith("http") ? "noreferrer noopener" : undefined}>{part.value}</a>;
    return <Fragment key={index}>{part.value}</Fragment>;
  })}</>;
}

function CodeBlock({ code, language }: { code: string; language: string }) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1600);
    } catch {
      setCopied(false);
    }
  };
  return <div className="md-code-block">
    <div className="md-code-header">
      <span>{language || "代码"}</span>
      <button type="button" className="md-copy-button" onClick={copy} title="复制代码" aria-label="复制代码">
        {copied ? <Check size={13} /> : <Copy size={13} />}{copied ? "已复制" : "复制"}
      </button>
    </div>
    <pre><code>{code}</code></pre>
  </div>;
}

export function MarkdownMessage({ content }: { content: string }) {
  const lines = content.replace(/\r\n?/g, "\n").split("\n");
  const blocks: Array<{ type: string; value: string; language?: string; level?: number }> = [];
  let paragraph: string[] = [];
  let list: string[] = [];
  let listType = "";
  const flushParagraph = () => {
    if (paragraph.length) blocks.push({ type: "paragraph", value: paragraph.join("\n") });
    paragraph = [];
  };
  const flushList = () => {
    if (list.length) blocks.push({ type: listType, value: list.join("\n") });
    list = [];
    listType = "";
  };
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const fence = line.match(/^\s*```([^\s`]*)\s*$/);
    if (fence) {
      flushParagraph(); flushList();
      const code: string[] = [];
      i++;
      while (i < lines.length && !/^\s*```\s*$/.test(lines[i])) code.push(lines[i++]);
      blocks.push({ type: "code", value: code.join("\n"), language: fence[1] });
      continue;
    }
    const heading = line.match(/^\s{0,3}(#{1,6})\s+(.+)$/);
    if (heading) {
      flushParagraph(); flushList();
      blocks.push({ type: "heading", value: heading[2], level: heading[1].length });
      continue;
    }
    if (line.includes("|") && i + 1 < lines.length && /^\s*\|?\s*:?-{3,}:?\s*(?:\|\s*:?-{3,}:?\s*)+\|?\s*$/.test(lines[i + 1])) {
      flushParagraph(); flushList();
      const tableLines = [line, lines[++i]];
      while (i + 1 < lines.length && lines[i + 1].includes("|") && lines[i + 1].trim()) tableLines.push(lines[++i]);
      blocks.push({ type: "table", value: tableLines.join("\n") });
      continue;
    }
    if (/^\s{0,3}([-*_])(?:\s*\1){2,}\s*$/.test(line)) {
      flushParagraph(); flushList(); blocks.push({ type: "hr", value: "" }); continue;
    }
    const bullet = line.match(/^\s*[-*+]\s+(.+)$/);
    const ordered = line.match(/^\s*\d+[.)]\s+(.+)$/);
    if (bullet || ordered) {
      flushParagraph();
      const type = bullet ? "ul" : "ol";
      if (listType && listType !== type) flushList();
      listType = type;
      list.push((bullet ?? ordered)![1]);
      continue;
    }
    if (/^\s*>\s?/.test(line)) {
      flushParagraph(); flushList();
      blocks.push({ type: "quote", value: line.replace(/^\s*>\s?/, "") });
      continue;
    }
    if (!line.trim()) {
      flushParagraph(); flushList(); continue;
    }
    flushList();
    paragraph.push(line);
  }
  flushParagraph(); flushList();

  return <div className="markdown-message">{blocks.map((block, index) => {
    if (block.type === "code") return <CodeBlock key={index} code={block.value} language={block.language ?? ""} />;
    if (block.type === "heading") {
      const text = <Inline source={block.value} />;
      if (block.level === 1) return <h1 key={index}>{text}</h1>;
      if (block.level === 2) return <h2 key={index}>{text}</h2>;
      if (block.level === 3) return <h3 key={index}>{text}</h3>;
      return <h4 key={index}>{text}</h4>;
    }
    if (block.type === "ul" || block.type === "ol") {
      const Tag = block.type;
      return <Tag key={index}>{block.value.split("\n").map((item, itemIndex) => <li key={itemIndex}><Inline source={item} /></li>)}</Tag>;
    }
    if (block.type === "table") {
      const rows = block.value.split("\n").map((row) => row.trim().replace(/^\||\|$/g, "").split("|").map((cell) => cell.trim()));
      const headers = rows[0] ?? [];
      const body = rows.slice(2);
      return <div className="md-table-wrap" key={index}><table className="md-table"><thead><tr>{headers.map((cell, cellIndex) => <th key={cellIndex}><Inline source={cell} /></th>)}</tr></thead><tbody>{body.map((row, rowIndex) => <tr key={rowIndex}>{headers.map((_, cellIndex) => <td key={cellIndex}><Inline source={row[cellIndex] ?? ""} /></td>)}</tr>)}</tbody></table></div>;
    }
    if (block.type === "quote") return <blockquote key={index}><Inline source={block.value} /></blockquote>;
    if (block.type === "hr") return <hr key={index} />;
    return <p key={index}>{block.value.split("\n").map((line, lineIndex) => <Fragment key={lineIndex}>{lineIndex > 0 && <br />}<Inline source={line} /></Fragment>)}</p>;
  })}</div>;
}
