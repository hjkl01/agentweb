import { useMemo, useState } from 'react';
import { ChevronDown, ChevronRight, Minus, Plus } from 'lucide-react';
import type { WorkspaceDiff } from '../types';

type Props = { diff?: WorkspaceDiff };
type DiffRow = { kind: 'hunk' | 'add' | 'remove' | 'context' | 'meta' | 'binary'; text: string; oldLine?: number; newLine?: number };
type DiffFile = { name: string; newName?: string; rows: DiffRow[]; status: 'added' | 'deleted' | 'modified' | 'renamed' | 'binary' };
type InlinePart = { text: string; changed: boolean };

function parseHunk(text: string) {
  const match = text.match(/^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
  return match ? { oldLine: Number(match[1]), newLine: Number(match[2]) } : undefined;
}

function unquoteGitPath(value: string) {
  if (!value.startsWith('"') || !value.endsWith('"')) return value;
  const body = value.slice(1, -1);
  let result = '';
  for (let i = 0; i < body.length; i += 1) {
    if (body[i] !== '\\') { result += body[i]; continue; }
    const next = body[++i];
    if (next === 'n') result += '\n';
    else if (next === 't') result += '\t';
    else if (next === 'r') result += '\r';
    else if (next === '\\' || next === '"') result += next;
    else if (/[0-7]/.test(next || '')) {
      let octal = next;
      while (octal.length < 3 && /[0-7]/.test(body[i + 1] || '')) octal += body[++i];
      result += String.fromCharCode(parseInt(octal, 8));
    } else result += next || '';
  }
  return result;
}

function parseGitHeader(line: string) {
  const prefix = 'diff --git ';
  if (!line.startsWith(prefix)) return undefined;
  const value = line.slice(prefix.length);
  let quote = false;
  let escaped = false;
  let separator = -1;
  for (let i = 0; i < value.length; i += 1) {
    const char = value[i];
    if (escaped) { escaped = false; continue; }
    if (char === '\\') { escaped = true; continue; }
    if (char === '"') { quote = !quote; continue; }
    if (!quote && char === ' ' && value.startsWith('b/', i + 1)) { separator = i; break; }
  }
  if (separator < 0) return undefined;
  return {
    oldPath: unquoteGitPath(value.slice(0, separator)).replace(/^a\//, ''),
    newPath: unquoteGitPath(value.slice(separator + 1)).replace(/^b\//, ''),
  };
}

function parseFiles(text?: string): DiffFile[] {
  if (!text?.trim()) return [];
  const files: DiffFile[] = [];
  let current: DiffFile | undefined;
  let oldLine = 0;
  let newLine = 0;
  for (const line of text.split('\n')) {
    if (line.startsWith('diff --git ')) {
      const header = parseGitHeader(line);
      current = { name: header?.oldPath || line.slice(11), newName: header?.newPath, rows: [], status: 'modified' };
      files.push(current);
      continue;
    }
    if (!current) continue;
    if (line.startsWith('new file mode')) current.status = 'added';
    else if (line.startsWith('deleted file mode')) current.status = 'deleted';
    else if (line.startsWith('Binary files ') || line.startsWith('GIT binary patch')) current.status = 'binary';
    else if (line.startsWith('similarity index') || line.startsWith('rename from') || line.startsWith('rename to')) current.status = 'renamed';
    if (line.startsWith('@@ ')) {
      const hunk = parseHunk(line);
      oldLine = hunk?.oldLine || 0;
      newLine = hunk?.newLine || 0;
      current.rows.push({ kind: 'hunk', text: line, oldLine, newLine });
    } else if (current.status === 'binary' && (line.startsWith('Binary files ') || line.startsWith('GIT binary patch'))) {
      current.rows.push({ kind: 'binary', text: line });
    } else if (line.startsWith('+') && !line.startsWith('+++')) current.rows.push({ kind: 'add', text: line.slice(1), newLine: newLine++ });
    else if (line.startsWith('-') && !line.startsWith('---')) current.rows.push({ kind: 'remove', text: line.slice(1), oldLine: oldLine++ });
    else if (/^(---|\+\+\+|index |new file mode|deleted file mode|similarity index|rename from|rename to)/.test(line)) current.rows.push({ kind: 'meta', text: line });
    else if (line && !line.startsWith('\\ No newline')) current.rows.push({ kind: 'context', text: line, oldLine: oldLine++, newLine: newLine++ });
  }
  return files;
}

function inlineDiff(oldText: string, newText: string): { oldParts: InlinePart[]; newParts: InlinePart[] } {
  let prefix = 0;
  while (prefix < oldText.length && prefix < newText.length && oldText[prefix] === newText[prefix]) prefix += 1;
  let suffix = 0;
  while (suffix < oldText.length - prefix && suffix < newText.length - prefix && oldText[oldText.length - suffix - 1] === newText[newText.length - suffix - 1]) suffix += 1;
  const oldMiddle = oldText.slice(prefix, oldText.length - suffix || undefined);
  const newMiddle = newText.slice(prefix, newText.length - suffix || undefined);
  const oldParts: InlinePart[] = [{ text: oldText.slice(0, prefix), changed: false }];
  const newParts: InlinePart[] = [{ text: newText.slice(0, prefix), changed: false }];
  if (oldMiddle) oldParts.push({ text: oldMiddle, changed: true });
  if (newMiddle) newParts.push({ text: newMiddle, changed: true });
  if (suffix) {
    oldParts.push({ text: oldText.slice(-suffix), changed: false });
    newParts.push({ text: newText.slice(-suffix), changed: false });
  }
  return { oldParts, newParts };
}

function InlineText({ parts, kind }: { parts: InlinePart[]; kind: 'add' | 'remove' }) {
  return <>{parts.map((part, index) => part.changed ? <mark key={index} style={{ background: kind === 'add' ? '#b9e8c7' : '#f5b9b3', borderRadius: 2, padding: '1px 0' }}>{part.text}</mark> : <span key={index}>{part.text}</span>)}</>;
}

function Row({ row, inlineParts }: { row: DiffRow; inlineParts?: InlinePart[] }) {
  const marker = row.kind === 'add' ? <Plus size={12} /> : row.kind === 'remove' ? <Minus size={12} /> : null;
  const backgrounds: Record<DiffRow['kind'], string> = { add: '#eaf8ee', remove: '#fff0ee', hunk: '#edf2ff', context: '#fafafa', meta: '#f8f8f8', binary: '#fff' };
  const lineBackground = row.kind === 'add' ? '#dff2e5' : row.kind === 'remove' ? '#ffe3df' : row.kind === 'hunk' ? '#e7edff' : '#f5f5f5';
  return <div className={`diff-row diff-${row.kind}`} style={{ display: 'grid', gridTemplateColumns: '36px 36px 18px minmax(max-content, 1fr)', minHeight: 20, lineHeight: '20px', whiteSpace: 'pre', background: backgrounds[row.kind] }}><span className="diff-line-number" style={{ paddingRight: 6, textAlign: 'right', color: row.kind === 'hunk' ? '#7181aa' : '#aaa', background: lineBackground, borderRight: '1px solid #eee', userSelect: 'none' }}>{row.oldLine || ''}</span><span className="diff-line-number" style={{ paddingRight: 6, textAlign: 'right', color: row.kind === 'hunk' ? '#7181aa' : '#aaa', background: lineBackground, borderRight: '1px solid #eee', userSelect: 'none' }}>{row.newLine || ''}</span><span className="diff-marker" style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', color: row.kind === 'add' ? '#2e8b57' : row.kind === 'remove' ? '#c24136' : '#999' }}>{marker}</span><code style={{ padding: '0 8px', overflow: 'visible', font: 'inherit' }}>{inlineParts ? <InlineText parts={inlineParts} kind={row.kind as 'add' | 'remove'} /> : row.text || ' '}</code></div>;
}

function DiffFileView({ file }: { file: DiffFile }) {
  const [open, setOpen] = useState(true);
  const additions = file.rows.filter(row => row.kind === 'add').length;
  const removals = file.rows.filter(row => row.kind === 'remove').length;
  const badge = file.status === 'added' ? 'A' : file.status === 'deleted' ? 'D' : file.status === 'renamed' ? 'R' : file.status === 'binary' ? 'B' : 'M';
  return <section className="diff-file"><button className="diff-file-head" onClick={() => setOpen(value => !value)}>{open ? <ChevronDown size={14} /> : <ChevronRight size={14} />}<span className={`diff-status-badge diff-status-${file.status}`}>{badge}</span><strong>{file.name}</strong>{file.newName && file.newName !== file.name && <span className="diff-renamed">→ {file.newName}</span>}<span className="diff-count">{additions ? `+${additions}` : ''}{removals ? ` -${removals}` : ''}</span></button>{open && <div className="diff-file-body">{file.status !== 'binary' && file.rows.map((row, index) => { const next = file.rows[index + 1]; if (row.kind === 'remove' && next?.kind === 'add') { const parts = inlineDiff(row.text, next.text); return <div key={`${index}-${row.text}`}><Row row={row} inlineParts={parts.oldParts} /><Row row={next} inlineParts={parts.newParts} /></div>; } if (row.kind === 'add' && file.rows[index - 1]?.kind === 'remove') return null; return <Row key={`${index}-${row.text}`} row={row} />; })}{file.status === 'binary' && <div className="diff-binary">Binary file cannot be displayed as text.</div>}</div>}</section>;
}

export function DiffViewer({ diff }: Props) {
  const files = useMemo(() => parseFiles(diff?.diff), [diff?.diff]);
  if (!files.length) return <div className="diff-empty">No git changes</div>;
  return <div className="diff-viewer">{diff?.status && <div className="diff-status">{diff.status}</div>}{diff?.truncated && <div className="diff-truncated">Diff is truncated because it exceeded the size limit.</div>}{files.map(file => <DiffFileView key={`${file.name}:${file.newName || ''}`} file={file} />)}</div>;
}
