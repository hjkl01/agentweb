import { useMemo, useState } from 'react';
import { ChevronDown, ChevronRight, Minus, Plus } from 'lucide-react';
import type { WorkspaceDiff } from '../types';

type Props = { diff?: WorkspaceDiff; onOpenFile?: (path: string, line?: number) => void };
type DiffRow = { kind: 'hunk' | 'add' | 'remove' | 'context' | 'meta' | 'binary'; text: string; oldLine?: number; newLine?: number };
type DiffFile = { name: string; newName?: string; rows: DiffRow[]; status: 'added' | 'deleted' | 'modified' | 'renamed' | 'binary' };

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
  const oldPath = unquoteGitPath(value.slice(0, separator)).replace(/^a\//, '');
  const newPath = unquoteGitPath(value.slice(separator + 1)).replace(/^b\//, '');
  return { oldPath, newPath };
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
      files.push(current); continue;
    }
    if (!current) continue;
    if (line.startsWith('new file mode')) current.status = 'added';
    else if (line.startsWith('deleted file mode')) current.status = 'deleted';
    else if (line.startsWith('Binary files ') || line.startsWith('GIT binary patch')) current.status = 'binary';
    else if (line.startsWith('similarity index') || line.startsWith('rename from') || line.startsWith('rename to')) current.status = 'renamed';
    if (line.startsWith('@@ ')) {
      const hunk = parseHunk(line); oldLine = hunk?.oldLine || 0; newLine = hunk?.newLine || 0;
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

function Row({ row, onOpen }: { row: DiffRow; onOpen?: (line?: number) => void }) {
  const marker = row.kind === 'add' ? <Plus size={12} /> : row.kind === 'remove' ? <Minus size={12} /> : null;
  const clickable = row.kind === 'add' || row.kind === 'remove' || row.kind === 'context';
  return <div className={`diff-row diff-${row.kind}${clickable ? ' diff-row-clickable' : ''}`} onClick={() => clickable && onOpen?.(row.newLine || row.oldLine)}><span className="diff-line-number">{row.oldLine || ''}</span><span className="diff-line-number">{row.newLine || ''}</span><span className="diff-marker">{marker}</span><code>{row.text || ' '}</code></div>;
}

function DiffFileView({ file, onOpenFile }: { file: DiffFile; onOpenFile?: (path: string, line?: number) => void }) {
  const [open, setOpen] = useState(true);
  const additions = file.rows.filter(row => row.kind === 'add').length;
  const removals = file.rows.filter(row => row.kind === 'remove').length;
  const badge = file.status === 'added' ? 'A' : file.status === 'deleted' ? 'D' : file.status === 'renamed' ? 'R' : file.status === 'binary' ? 'B' : 'M';
  const target = file.status === 'deleted' ? file.name : file.newName || file.name;
  const canOpen = file.status !== 'binary';
  const openLine = (line?: number) => canOpen && onOpenFile?.(target, line);
  return <section className="diff-file">
    <button className="diff-file-head" onClick={() => setOpen(value => !value)}>{open ? <ChevronDown size={14} /> : <ChevronRight size={14} />}<span className={`diff-status-badge diff-status-${file.status}`}>{badge}</span><strong>{file.name}</strong>{file.newName && file.newName !== file.name && <span className="diff-renamed">→ {file.newName}</span>}<span className="diff-count">{additions ? `+${additions}` : ''}{removals ? ` -${removals}` : ''}</span></button>
    {open && <div className="diff-file-body">{file.status !== 'binary' && file.rows.map((row, index) => <Row key={`${index}-${row.text}`} row={row} onOpen={openLine} />)}{file.status === 'binary' && <div className="diff-binary">Binary file cannot be displayed as text.</div>}{canOpen && <button className="diff-open-file" onClick={() => openLine()}>Open {target}</button>}</div>}
  </section>;
}

export function DiffViewer({ diff, onOpenFile }: Props) {
  const files = useMemo(() => parseFiles(diff?.diff), [diff?.diff]);
  if (!files.length) return <div className="diff-empty">No git changes</div>;
  return <div className="diff-viewer">{diff?.status && <div className="diff-status">{diff.status}</div>}{diff?.truncated && <div className="diff-truncated">Diff is truncated because it exceeded the size limit.</div>}{files.map(file => <DiffFileView key={`${file.name}:${file.newName || ''}`} file={file} onOpenFile={onOpenFile} />)}</div>;
}
