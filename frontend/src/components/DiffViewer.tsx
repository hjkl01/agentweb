import { useMemo, useState } from 'react';
import { ChevronDown, ChevronRight, Minus, Plus } from 'lucide-react';
import type { WorkspaceDiff } from '../types';

type Props = { diff?: WorkspaceDiff; onOpenFile?: (path: string) => void };
type DiffRow = { kind: 'hunk' | 'add' | 'remove' | 'context' | 'meta' | 'binary'; text: string; oldLine?: number; newLine?: number };
type DiffFile = { name: string; newName?: string; rows: DiffRow[]; status: 'added' | 'deleted' | 'modified' | 'renamed' | 'binary' };

function parseHunk(text: string) {
  const match = text.match(/^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
  return match ? { oldLine: Number(match[1]), newLine: Number(match[2]) } : undefined;
}

function parseFiles(text?: string): DiffFile[] {
  if (!text?.trim()) return [];
  const files: DiffFile[] = [];
  let current: DiffFile | undefined;
  let oldLine = 0;
  let newLine = 0;
  for (const line of text.split('\n')) {
    if (line.startsWith('diff --git ')) {
      const match = line.match(/^diff --git a\/(.+) b\/(.+)$/);
      current = { name: match?.[1] || line.slice(11), newName: match?.[2], rows: [], status: 'modified' };
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
      oldLine = hunk?.oldLine || 0; newLine = hunk?.newLine || 0;
      current.rows.push({ kind: 'hunk', text: line, oldLine, newLine });
    } else if (current.status === 'binary' && (line.startsWith('Binary files ') || line.startsWith('GIT binary patch'))) {
      current.rows.push({ kind: 'binary', text: line });
    } else if (line.startsWith('+') && !line.startsWith('+++')) {
      current.rows.push({ kind: 'add', text: line.slice(1), newLine: newLine++ });
    } else if (line.startsWith('-') && !line.startsWith('---')) {
      current.rows.push({ kind: 'remove', text: line.slice(1), oldLine: oldLine++ });
    } else if (/^(---|\+\+\+|index |new file mode|deleted file mode|similarity index|rename from|rename to)/.test(line)) {
      current.rows.push({ kind: 'meta', text: line });
    } else if (line && !line.startsWith('\\ No newline')) {
      current.rows.push({ kind: 'context', text: line.slice(0), oldLine: oldLine++, newLine: newLine++ });
    }
  }
  return files;
}

function Row({ row }: { row: DiffRow }) {
  const marker = row.kind === 'add' ? <Plus size={12} /> : row.kind === 'remove' ? <Minus size={12} /> : null;
  return <div className={`diff-row diff-${row.kind}`}><span className="diff-line-number">{row.oldLine || ''}</span><span className="diff-line-number">{row.newLine || ''}</span><span className="diff-marker">{marker}</span><code>{row.text || ' '}</code></div>;
}

function DiffFileView({ file, onOpenFile }: { file: DiffFile; onOpenFile?: (path: string) => void }) {
  const [open, setOpen] = useState(true);
  const additions = file.rows.filter(row => row.kind === 'add').length;
  const removals = file.rows.filter(row => row.kind === 'remove').length;
  const badge = file.status === 'added' ? 'A' : file.status === 'deleted' ? 'D' : file.status === 'renamed' ? 'R' : file.status === 'binary' ? 'B' : 'M';
  const target = file.status === 'deleted' ? file.name : file.newName || file.name;
  return <section className="diff-file">
    <button className="diff-file-head" onClick={() => setOpen(value => !value)}>
      {open ? <ChevronDown size={14} /> : <ChevronRight size={14} />}<span className={`diff-status-badge diff-status-${file.status}`}>{badge}</span><strong>{file.name}</strong>{file.newName && file.newName !== file.name && <span className="diff-renamed">→ {file.newName}</span>}
      <span className="diff-count">{additions ? `+${additions}` : ''}{removals ? ` -${removals}` : ''}</span>
    </button>
    {open && <div className="diff-file-body">{file.status !== 'binary' && file.rows.map((row, index) => <Row key={`${index}-${row.text}`} row={row} />)}{file.status === 'binary' && <div className="diff-binary">Binary file cannot be displayed as text.</div>}<button className="diff-open-file" onClick={() => onOpenFile?.(target)} disabled={!onOpenFile}>Open {target}</button></div>}
  </section>;
}

export function DiffViewer({ diff, onOpenFile }: Props) {
  const files = useMemo(() => parseFiles(diff?.diff), [diff?.diff]);
  if (!files.length) return <div className="diff-empty">No git changes</div>;
  return <div className="diff-viewer">{diff?.status && <div className="diff-status">{diff.status}</div>}{files.map(file => <DiffFileView key={`${file.name}:${file.newName || ''}`} file={file} onOpenFile={onOpenFile} />)}</div>;
}
