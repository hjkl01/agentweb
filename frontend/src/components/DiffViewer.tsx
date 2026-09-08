import { useMemo, useState } from 'react';
import { ChevronDown, ChevronRight, Minus, Plus } from 'lucide-react';
import type { WorkspaceDiff } from '../types';

type Props = { diff?: WorkspaceDiff };
type DiffRow = { kind: 'hunk' | 'add' | 'remove' | 'context' | 'meta'; text: string };
type DiffFile = { name: string; rows: DiffRow[] };

function parseFiles(text?: string): DiffFile[] {
  if (!text?.trim()) return [];
  const files: DiffFile[] = [];
  let current: DiffFile | undefined;
  for (const line of text.split('\n')) {
    if (line.startsWith('diff --git ')) {
      current = { name: line.replace(/^diff --git a\//, '').replace(/ b\/.*$/, ''), rows: [] };
      files.push(current);
      continue;
    }
    if (!current) continue;
    if (line.startsWith('@@ ')) current.rows.push({ kind: 'hunk', text: line });
    else if (line.startsWith('+') && !line.startsWith('+++')) current.rows.push({ kind: 'add', text: line });
    else if (line.startsWith('-') && !line.startsWith('---')) current.rows.push({ kind: 'remove', text: line });
    else if (/^(---|\+\+\+|index |new file mode|deleted file mode|similarity index)/.test(line)) current.rows.push({ kind: 'meta', text: line });
    else current.rows.push({ kind: 'context', text: line });
  }
  return files;
}

function Row({ row }: { row: DiffRow }) {
  const marker = row.kind === 'add' ? <Plus size={12} /> : row.kind === 'remove' ? <Minus size={12} /> : null;
  return <div className={`diff-row diff-${row.kind}`}><span className="diff-marker">{marker}</span><code>{row.text || ' '}</code></div>;
}

function DiffFileView({ file }: { file: DiffFile }) {
  const [open, setOpen] = useState(true);
  const additions = file.rows.filter(row => row.kind === 'add').length;
  const removals = file.rows.filter(row => row.kind === 'remove').length;
  return <section className="diff-file">
    <button className="diff-file-head" onClick={() => setOpen(value => !value)}>
      {open ? <ChevronDown size={14} /> : <ChevronRight size={14} />}<strong>{file.name}</strong>
      <span className="diff-count">{additions ? `+${additions}` : ''}{removals ? ` -${removals}` : ''}</span>
    </button>
    {open && <div className="diff-file-body">{file.rows.map((row, index) => <Row key={`${index}-${row.text}`} row={row} />)}</div>}
  </section>;
}

export function DiffViewer({ diff }: Props) {
  const files = useMemo(() => parseFiles(diff?.diff), [diff?.diff]);
  if (!files.length) return <div className="diff-empty">No git changes</div>;
  return <div className="diff-viewer">{diff?.status && <div className="diff-status">{diff.status}</div>}{files.map(file => <DiffFileView key={file.name} file={file} />)}</div>;
}
