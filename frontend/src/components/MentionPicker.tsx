import { Check, ChevronDown, ChevronRight, File, Folder, FolderOpen, LoaderCircle, Search } from 'lucide-react';
import { useEffect, useState } from 'react';
import { api } from '../lib/api';

type Item = { name: string; path: string; kind: string; size: number };
type Props = { open: boolean; query: string; sessionId: string; onSelect: (path: string) => void; onClose: () => void };
type TreeNodeProps = { item: Item; depth: number; sessionId: string; onSelect: (path: string) => void };

function TreeNode({ item, depth, sessionId, onSelect }: TreeNodeProps) {
  const [expanded, setExpanded] = useState(false);
  const [children, setChildren] = useState<Item[]>([]);
  const [loading, setLoading] = useState(false);
  const directory = item.kind === 'directory';
  const toggle = async () => {
    if (!directory) return;
    if (expanded) { setExpanded(false); return; }
    setLoading(true);
    try { setChildren(await api<Item[]>(`/sessions/${sessionId}/filesystem/tree?path=${encodeURIComponent(item.path)}`)); setExpanded(true); }
    catch { setChildren([]); }
    finally { setLoading(false); }
  };
  return <div className="mention-tree-node">
    <div className="mention-tree-row" style={{ paddingLeft: depth * 18 }}>
      <span className="mention-tree-name">{directory ? (expanded ? <FolderOpen size={15} /> : <Folder size={15} />) : <File size={14} />}<span title={item.path}>{item.name}</span>{loading && <LoaderCircle size={13} className="spin" />}</span>
      {directory ? <button className="mention-expand" onClick={toggle} title={expanded ? `折叠 ${item.name}` : `展开 ${item.name}`} aria-label={expanded ? `折叠 ${item.name}` : `展开 ${item.name}`}>{expanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}</button> : <span className="mention-expand-placeholder" />}
      <button className="mention-confirm" onClick={() => onSelect(item.path)} title={`选择 ${item.name}`} aria-label={`选择 ${item.name}`}><Check size={14} /><span>确定</span></button>
    </div>
    {expanded && children.map(child => <TreeNode key={child.path} item={child} depth={depth + 1} sessionId={sessionId} onSelect={onSelect} />)}
  </div>;
}

export function MentionPicker({ open, query, sessionId, onSelect, onClose }: Props) {
  const [root, setRoot] = useState<Item[]>([]);
  const [results, setResults] = useState<Item[]>([]);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!open) return;
    setLoading(true);
    api<Item[]>(`/sessions/${sessionId}/filesystem/tree`)
      .then(setRoot).catch(() => setRoot([])).finally(() => setLoading(false));
  }, [open, sessionId]);

  useEffect(() => {
    if (!open || !query.trim()) { setResults([]); return; }
    const timer = window.setTimeout(() => {
      setLoading(true);
      api<Item[]>(`/sessions/${sessionId}/filesystem/tree?search=${encodeURIComponent(query.trim())}`)
        .then(setResults).catch(() => setResults([])).finally(() => setLoading(false));
    }, 120);
    return () => window.clearTimeout(timer);
  }, [open, query, sessionId]);

  if (!open) return null;
  const visible = query.trim() ? results : root;
  return <div className="mention-picker" onMouseDown={event => event.preventDefault()}>
    <div className="mention-picker-head"><div><strong>选择文件或文件夹</strong><small>{query.trim() ? '搜索整个工作区' : '从当前工作区开始，按需展开'}</small></div><button onClick={onClose}>Esc</button></div>
    <div className="mention-search"><Search size={14} /><span>@{query || '输入名称搜索'}</span></div>
    <div className="mention-tree">
      {loading ? <div className="mention-loading"><LoaderCircle size={16} className="spin" />搜索工作区…</div> : visible.length ? visible.map(item => <TreeNode key={item.path} item={item} depth={0} sessionId={sessionId} onSelect={onSelect} />) : <div className="mention-empty">没有匹配的文件或文件夹</div>}
    </div>
  </div>;
}
