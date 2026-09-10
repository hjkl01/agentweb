import { useEffect, useMemo, useState } from 'react';
import { api } from '../lib/api';

interface Item {
  name: string;
  path: string;
  kind: 'directory' | 'file';
  size: number;
}

interface MentionPickerProps {
  open: boolean;
  query: string;
  sessionId: string;
  onSelect: (path: string) => void;
  onClose: () => void;
}

interface TreeNodeProps {
  item: Item;
  sessionId: string;
  query: string;
  onSelect: (path: string) => void;
}

function TreeNode({ item, sessionId, query, onSelect }: TreeNodeProps) {
  const [expanded, setExpanded] = useState(false);
  const [children, setChildren] = useState<Item[] | null>(null);
  const [loading, setLoading] = useState(false);

  const toggle = async () => {
    if (item.kind !== 'directory') {
      onSelect(item.path);
      return;
    }

    if (expanded) {
      setExpanded(false);
      return;
    }

    setExpanded(true);
    if (children) return;

    setLoading(true);
    try {
      const data = await api<Item[]>(
        `/sessions/${sessionId}/filesystem/tree?path=${encodeURIComponent(item.path)}`,
      );
      setChildren(data);
    } finally {
      setLoading(false);
    }
  };

  const filteredChildren = useMemo(() => {
    if (!children) return null;
    const normalized = query.trim().toLowerCase();
    if (!normalized) return children;
    return children.filter((child) =>
      child.name.toLowerCase().includes(normalized),
    );
  }, [children, query]);

  return (
    <div>
      <button type="button" onClick={toggle} className="mention-tree-item">
        <span>{item.kind === 'directory' ? (expanded ? '▾' : '▸') : '•'}</span>
        <span>{item.name}</span>
      </button>
      {expanded && (
        <div className="mention-tree-children">
          {loading && <div className="mention-tree-loading">加载中…</div>}
          {filteredChildren?.map((child) => (
            <TreeNode
              key={child.path}
              item={child}
              sessionId={sessionId}
              query={query}
              onSelect={onSelect}
            />
          ))}
        </div>
      )}
    </div>
  );
}

export default function MentionPicker({
  open,
  query,
  sessionId,
  onSelect,
  onClose,
}: MentionPickerProps) {
  const [items, setItems] = useState<Item[]>([]);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!open || !sessionId) return;

    let cancelled = false;
    setLoading(true);
    api<Item[]>(`/sessions/${sessionId}/filesystem/tree`)
      .then((data) => {
        if (!cancelled) setItems(data);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [open, sessionId]);

  const filteredItems = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    if (!normalized) return items;
    return items.filter((item) =>
      item.name.toLowerCase().includes(normalized),
    );
  }, [items, query]);

  if (!open) return null;

  return (
    <div className="mention-picker" role="dialog" aria-label="选择文件">
      <div className="mention-picker-header">
        <span>选择文件</span>
        <button type="button" onClick={onClose} aria-label="关闭">
          ×
        </button>
      </div>
      <div className="mention-picker-hint">从当前工作区开始，按需展开</div>
      <div className="mention-picker-body">
        {loading && <div className="mention-tree-loading">加载当前工作区…</div>}
        {!loading &&
          filteredItems.map((item) => (
            <TreeNode
              key={item.path}
              item={item}
              sessionId={sessionId}
              query={query}
              onSelect={onSelect}
            />
          ))}
        {!loading && filteredItems.length === 0 && (
          <div className="mention-tree-loading">没有匹配的文件</div>
        )}
      </div>
    </div>
  );
}
