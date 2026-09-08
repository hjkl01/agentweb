import { ChevronDown, ChevronRight, File, Folder } from 'lucide-react';
import { useMemo, useState } from 'react';
import type { FileItem } from '../types';

type TreeNode = {
  name: string;
  path: string;
  kind: 'file' | 'directory';
  size?: number;
  children: TreeNode[];
};

type Props = {
  files: FileItem[];
  filter: string;
  onOpenFile: (path: string) => void;
};

function buildTree(files: FileItem[]): TreeNode[] {
  const root: TreeNode[] = [];
  for (const file of files) {
    const parts = file.path.split('/').filter(Boolean);
    let children = root;
    let currentPath = '';

    parts.forEach((name, index) => {
      currentPath = currentPath ? `${currentPath}/${name}` : name;
      const isLast = index === parts.length - 1;
      let node = children.find(item => item.name === name);
      if (!node) {
        node = {
          name,
          path: currentPath,
          kind: isLast && file.kind !== 'directory' ? 'file' : 'directory',
          size: isLast ? file.size : undefined,
          children: [],
        };
        children.push(node);
      }
      children = node.children;
    });
  }

  const sort = (nodes: TreeNode[]) => {
    nodes.sort((a, b) => a.kind !== b.kind ? (a.kind === 'directory' ? -1 : 1) : a.name.localeCompare(b.name));
    nodes.forEach(node => sort(node.children));
  };
  sort(root);
  return root;
}

function filterTree(nodes: TreeNode[], filter: string): TreeNode[] {
  if (!filter) return nodes;
  const query = filter.toLowerCase();
  return nodes.flatMap(node => {
    const children = filterTree(node.children, filter);
    return node.path.toLowerCase().includes(query) || children.length ? [{ ...node, children }] : [];
  });
}

function collectDirectories(nodes: TreeNode[], result = new Set<string>()) {
  nodes.forEach(node => {
    if (node.kind === 'directory') {
      result.add(node.path);
      collectDirectories(node.children, result);
    }
  });
  return result;
}

function TreeNodeView({ node, depth, expanded, toggle, onOpenFile }: {
  node: TreeNode;
  depth: number;
  expanded: Set<string>;
  toggle: (path: string) => void;
  onOpenFile: (path: string) => void;
}) {
  const isOpen = expanded.has(node.path);
  if (node.kind === 'file') {
    return <button className="file tree-file" style={{ paddingLeft: 7 + depth * 14 }} onClick={() => onOpenFile(node.path)}><File size={14} /><span>{node.name}</span></button>;
  }
  return (
    <div className="tree-node">
      <button className="file tree-folder" style={{ paddingLeft: 5 + depth * 14 }} onClick={() => toggle(node.path)}>
        {isOpen ? <ChevronDown size={14} /> : <ChevronRight size={14} />}<Folder size={14} /><span>{node.name}</span>
      </button>
      {isOpen && node.children.map(child => <TreeNodeView key={child.path} node={child} depth={depth + 1} expanded={expanded} toggle={toggle} onOpenFile={onOpenFile} />)}
    </div>
  );
}

export function FileTree({ files, filter, onOpenFile }: Props) {
  const tree = useMemo(() => filterTree(buildTree(files), filter), [files, filter]);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const visibleExpanded = filter ? collectDirectories(tree) : expanded;

  const toggle = (path: string) => {
    setExpanded(current => {
      const next = new Set(current);
      if (next.has(path)) next.delete(path); else next.add(path);
      return next;
    });
  };

  if (!tree.length) return <div className="workspace-empty">No files</div>;
  return <div className="filetree">{tree.map(node => <TreeNodeView key={node.path} node={node} depth={0} expanded={visibleExpanded} toggle={toggle} onOpenFile={onOpenFile} />)}</div>;
}
