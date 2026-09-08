import { useMemo } from 'react';
import { Check, Minus, Plus } from 'lucide-react';
import type { WorkspaceDiff } from '../types';

type Props = { diff?: WorkspaceDiff };
type DiffRow = { kind: 'file' | 'hunk' | 'add' | 'remove' | 'context' | 'meta'; text: string };

function rows(diff?: string): DiffRow[] {
  if (!diff?.trim()) return [];
  return diff.split('\n').map(text => {
    if (text.startsWith('diff --git ')) return { kind: 'file', text };
    if (text.startsWith('@@ ')) return { kind: 'hunk', text };
    if (text.startsWith('+') && !text.startsWith('+++')) return { kind: 'add', text };
    if (text.startsWith('-') && !text.startsWith('---')) return { kind: 'remove', text };
    if (/^(---|\+\+\+|index |new file mode|deleted file mode|similarity index)/.test(text)) return { kind: 'meta', text };
    return { kind: 'context', text };
  });
}

function icon(kind: DiffRow['kind']) {
  if (kind === 'add') return <Plus size={12} />;
  if (kind === 'remove') return <Minus size={12} />;
  if (kind === 'file') return <Check size={12} />;
  return null;
}

export function DiffViewer({ diff }: Props) {
  const parsed = useMemo(() => rows(diff?.diff), [diff?.diff]);
  if (!parsed.length) return <div className="diff-empty">No git changes</div>;

  return (
    <div className="diff-viewer">
      {diff?.status && <div className="diff-status">{diff.status}</div>}
      {parsed.map((row, index) => (
        <div className={`diff-row diff-${row.kind}`} key={`${index}-${row.text}`}>
          <span className="diff-marker">{icon(row.kind)}</span>
          <code>{row.text || ' '}</code>
        </div>
      ))}
    </div>
  );
}
