import { ChevronDown, ChevronRight, File, Folder } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { api } from "../lib/api";
import type { FileItem } from "../types";

type TreeNode = {
  name: string;
  path: string;
  kind: "file" | "directory";
  size?: number;
  status?: FileItem["status"];
  children: TreeNode[];
};
type Props = {
  sessionId?: string;
  files: FileItem[];
  filter: string;
  onOpenFile: (path: string) => void;
};

function buildTree(files: FileItem[]) {
  const root: TreeNode[] = [];
  for (const file of files) {
    const parts = file.path.split("/").filter(Boolean);
    let children = root;
    let currentPath = "";
    parts.forEach((name, index) => {
      currentPath = currentPath ? `${currentPath}/${name}` : name;
      const last = index === parts.length - 1;
      let node = children.find((item) => item.name === name);
      if (!node) {
        node = {
          name,
          path: currentPath,
          kind: last && file.kind !== "directory" ? "file" : "directory",
          size: last ? file.size : undefined,
          status: last ? file.status : undefined,
          children: [],
        };
        children.push(node);
      } else if (last) {
        node.kind = file.kind === "directory" ? "directory" : node.kind;
        node.size = file.size;
        node.status = file.status;
      }
      children = node.children;
    });
  }
  sortNodes(root);
  return root;
}

function sortNodes(nodes: TreeNode[]) {
  nodes.sort((a, b) =>
    a.kind !== b.kind
      ? a.kind === "directory" ? -1 : 1
      : a.name.localeCompare(b.name),
  );
  nodes.forEach((node) => sortNodes(node.children));
}

function filterTree(nodes: TreeNode[], filter: string): TreeNode[] {
  if (!filter) return nodes;
  const query = filter.toLowerCase();
  return nodes.flatMap((node) => {
    const children = filterTree(node.children, filter);
    return node.path.toLowerCase().includes(query) || children.length
      ? [{ ...node, children }]
      : [];
  });
}

function collectDirectories(nodes: TreeNode[], result = new Set<string>()) {
  nodes.forEach((node) => {
    if (node.kind === "directory") {
      result.add(node.path);
      collectDirectories(node.children, result);
    }
  });
  return result;
}

function statusLabel(status?: TreeNode["status"]) {
  if (!status) return null;
  const labels = { added: "A", modified: "M", deleted: "D", renamed: "R", untracked: "U" };
  return <span className={`tree-status tree-status-${status}`}>{labels[status]}</span>;
}

function TreeNodeView({
  node, depth, expanded, toggle, onOpenFile,
}: {
  node: TreeNode;
  depth: number;
  expanded: Set<string>;
  toggle: (path: string) => void;
  onOpenFile: (path: string) => void;
}) {
  const isOpen = expanded.has(node.path);
  if (node.kind === "file") {
    return (
      <button className="file tree-file" style={{ paddingLeft: 7 + depth * 14 }} onClick={() => onOpenFile(node.path)}>
        <File size={14} />
        <span>{node.name}</span>
        {statusLabel(node.status)}
      </button>
    );
  }
  return (
    <div className="tree-node">
      <button className="file tree-folder" style={{ paddingLeft: 5 + depth * 14 }} onClick={() => toggle(node.path)}>
        {isOpen ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
        <Folder size={14} />
        <span>{node.name}</span>
        {statusLabel(node.status)}
      </button>
      {isOpen && node.children.map((child) => (
        <TreeNodeView key={child.path} node={child} depth={depth + 1} expanded={expanded} toggle={toggle} onOpenFile={onOpenFile} />
      ))}
    </div>
  );
}

export function FileTree({ sessionId, files, filter, onOpenFile }: Props) {
  const [allFiles, setAllFiles] = useState<FileItem[]>(files);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [loadedDirs, setLoadedDirs] = useState<Set<string>>(new Set([""]));
  const [loadingDirs, setLoadingDirs] = useState<Set<string>>(new Set());
  const [searchFiles, setSearchFiles] = useState<FileItem[]>();
  const searchRequest = useRef(0);

  const previousSessionId = useRef(sessionId);
  const sessionIdRef = useRef(sessionId);
  const loadedDirsRef = useRef(loadedDirs);
  const loadingDirsRef = useRef(loadingDirs);
  const refreshRequest = useRef(0);
  const directoryRequest = useRef(new Map<string, number>());
  loadedDirsRef.current = loadedDirs;
  sessionIdRef.current = sessionId;
  loadingDirsRef.current = loadingDirs;

  useEffect(() => {
    const request = ++refreshRequest.current;
    const sessionChanged = previousSessionId.current !== sessionId;
    previousSessionId.current = sessionId;

    if (sessionChanged) {
      setAllFiles(files);
      setExpanded(new Set());
      setLoadedDirs(new Set([""]));
      loadedDirsRef.current = new Set([""]);
      setLoadingDirs(new Set());
      loadingDirsRef.current = new Set();
      directoryRequest.current.clear();
      setSearchFiles(undefined);
      return;
    }

    // Re-fetch every directory previously loaded. This keeps expansion state
    // while ensuring deleted/renamed children do not survive a refresh.
    const directories = [...loadedDirsRef.current].filter(Boolean).sort((a, b) => a.split("/").length - b.split("/").length);
    if (!sessionId || directories.length === 0) {
      setAllFiles(files);
      return;
    }

    let cancelled = false;
    Promise.all(
      directories.map(async (path) => {
        const directoryToken = (directoryRequest.current.get(path) ?? 0) + 1;
        directoryRequest.current.set(path, directoryToken);
        try {
          const children = await api<FileItem[]>(
            `/sessions/${sessionId}/files?path=${encodeURIComponent(path)}`,
          );
          if (directoryRequest.current.get(path) !== directoryToken) return undefined;
          return { path, children };
        } catch {
          // A failed refresh is not an empty directory. Preserve the cached
          // branch instead of deleting its contents on transient API errors.
          if (directoryRequest.current.get(path) !== directoryToken) return undefined;
          return undefined;
        }
      }),
    ).then((results) => {
      if (cancelled || request !== refreshRequest.current) return;
      const merged = new Map(files.map((item) => [item.path, item]));
      results.forEach((result) => {
        if (!result) return;
        const { path, children } = result;
        // Replace only successfully refreshed branches; failed branches keep
        // their cached contents until a later refresh succeeds.
        for (const existingPath of merged.keys()) {
          if (existingPath.startsWith(`${path}/`)) merged.delete(existingPath);
        }
        children.forEach((item) => merged.set(item.path, item));
      });
      setAllFiles([...merged.values()]);
    });

    return () => {
      cancelled = true;
    };
  }, [files, sessionId]);

  useEffect(() => {
    const request = ++searchRequest.current;
    const query = filter.trim();
    if (!query || !sessionId) {
      setSearchFiles(undefined);
      return;
    }
    api<FileItem[]>(`/sessions/${sessionId}/filesystem/tree?search=${encodeURIComponent(query)}`)
      .then((items) => {
        if (request === searchRequest.current) setSearchFiles(items);
      })
      .catch(() => {
        if (request === searchRequest.current) setSearchFiles([]);
      });
    return () => { searchRequest.current += 1; };
  }, [filter, sessionId]);

  const sourceFiles = searchFiles ?? allFiles;
  const fullTree = useMemo(() => buildTree(sourceFiles), [sourceFiles]);
  const tree = useMemo(() => filterTree(fullTree, searchFiles ? "" : filter), [fullTree, filter, searchFiles]);
  const visibleExpanded = searchFiles ? collectDirectories(tree) : expanded;

  const toggle = async (path: string) => {
    if (expanded.has(path)) {
      setExpanded((current) => {
        const next = new Set(current);
        next.delete(path);
        return next;
      });
      return;
    }
    setExpanded((current) => new Set(current).add(path));
    if (loadedDirs.has(path) || !sessionId || searchFiles) return;

    // A previous request for this path may still be in flight. Avoid duplicate
    // requests, and never let a response from an old session mutate the new tree.
    if (loadingDirsRef.current.has(path)) return;
    const requestSessionId = sessionId;
    const directoryToken = (directoryRequest.current.get(path) ?? 0) + 1;
    directoryRequest.current.set(path, directoryToken);
    const nextLoading = new Set(loadingDirsRef.current);
    nextLoading.add(path);
    loadingDirsRef.current = nextLoading;
    setLoadingDirs(nextLoading);

    try {
      const children = await api<FileItem[]>(
        `/sessions/${requestSessionId}/files?path=${encodeURIComponent(path)}`,
      );
      if (
        sessionIdRef.current !== requestSessionId ||
        directoryRequest.current.get(path) !== directoryToken
      ) return;

      setAllFiles((current) => {
        const byPath = new Map(current.map((item) => [item.path, item]));
        // Replace the directory's direct children rather than retaining files
        // that disappeared while the request was in flight.
        for (const existingPath of byPath.keys()) {
          if (existingPath.startsWith(`${path}/`)) byPath.delete(existingPath);
        }
        children.forEach((item) => byPath.set(item.path, item));
        return [...byPath.values()];
      });
      setLoadedDirs((current) => new Set(current).add(path));
    } catch {
      if (
        sessionIdRef.current !== requestSessionId ||
        directoryRequest.current.get(path) !== directoryToken
      ) return;
      setExpanded((current) => {
        const next = new Set(current);
        next.delete(path);
        return next;
      });
    } finally {
      if (
        sessionIdRef.current === requestSessionId &&
        directoryRequest.current.get(path) === directoryToken
      ) {
        const next = new Set(loadingDirsRef.current);
        next.delete(path);
        loadingDirsRef.current = next;
        setLoadingDirs(next);
      }
    }
  };

  if (!tree.length) return <div className="workspace-empty">{filter && searchFiles === undefined ? "Searching…" : "No files"}</div>;
  return (
    <div className="filetree">
      {tree.map((node) => (
        <TreeNodeView key={node.path} node={node} depth={0} expanded={visibleExpanded} toggle={toggle} onOpenFile={onOpenFile} />
      ))}
      {loadingDirs.size > 0 && <div className="workspace-empty">Loading folder…</div>}
    </div>
  );
}
