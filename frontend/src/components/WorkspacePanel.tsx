import {
  AlertTriangle,
  FileWarning,
  Folder,
  GitCompare,
  Maximize2,
  Minimize2,
  RefreshCw,
  Search,
  X,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { CodePreview } from "./CodePreview";
import { DiffViewer } from "./DiffViewer";
import { FileTree } from "./FileTree";
import type { FileItem, Session, WorkspaceDiff, WorkspaceFile } from "../types";

type Props = {
  current?: Session;
  files: FileItem[];
  fileFilter: string;
  selectedFile?: WorkspaceFile;
  selectedLine?: number;
  diff?: WorkspaceDiff;
  tab: "files" | "diff";
  onFilterChange: (value: string) => void;
  onSelectTab: (tab: "files" | "diff") => void;
  onRefresh: () => void;
  onOpenFile: (path: string, line?: number) => void;
  onCloseFile: () => void;
};

function PreviewNotice({ file }: { file: WorkspaceFile }) {
  if (!file.binary && !file.truncated) return null;
  return (
    <div className="preview-notice">
      <span>
        {file.binary ? <FileWarning size={15} /> : <AlertTriangle size={15} />}
      </span>
      <span>
        {file.message ||
          (file.binary
            ? "Binary file preview is not supported."
            : "File preview was truncated.")}
      </span>
    </div>
  );
}

function Preview({
  file,
  line,
  onClose,
  onRetry,
  maximized,
  onToggleMaximize,
}: {
  file: WorkspaceFile;
  line?: number;
  onClose: () => void;
  onRetry: () => void;
  maximized: boolean;
  onToggleMaximize: () => void;
}) {
  const unavailable = file.binary || file.truncated;
  return (
    <div className="preview">
      <div className="preview-head">
        <div>
          <strong>{file.path}</strong>
          <small>
            File preview{line ? ` · line ${line}` : ""}
            {file.size !== undefined
              ? ` · ${file.size.toLocaleString()} bytes`
              : ""}
            {file.source === "git" ? " · HEAD" : ""}
          </small>
        </div>
        <div className="preview-head-actions">
          <button
            className="icon-button"
            onClick={onToggleMaximize}
            title={maximized ? "Restore preview" : "Maximize preview"}
            aria-label={maximized ? "Restore preview" : "Maximize preview"}
          >
            {maximized ? <Minimize2 size={15} /> : <Maximize2 size={15} />}
          </button>
          <button
            className="icon-button"
            onClick={onClose}
            title="Close preview"
            aria-label="Close preview"
          >
            <X size={15} />
          </button>
        </div>
      </div>
      {file.error ? (
        <div className="preview-error" role="alert">
          <AlertTriangle size={18} />
          <p>{file.error}</p>
          <button onClick={onRetry}>Retry</button>
        </div>
      ) : (
        <>
          <PreviewNotice file={file} />
          {!unavailable && <CodePreview file={file} focusLine={line} />}
        </>
      )}
    </div>
  );
}

export function WorkspacePanel({
  current,
  files,
  fileFilter,
  selectedFile,
  selectedLine,
  diff,
  tab,
  onFilterChange,
  onSelectTab,
  onRefresh,
  onOpenFile,
  onCloseFile,
}: Props) {
  const [openTabs, setOpenTabs] = useState<string[]>([]);
  const [activePath, setActivePath] = useState<string>();
  const [maximized, setMaximized] = useState(false);
  const closedPathsRef = useRef(new Set<string>());
  const previousSessionId = useRef(current?.id);

  useEffect(() => {
    closedPathsRef.current.clear();
    setOpenTabs([]);
    setActivePath(undefined);
    setMaximized(false);
  }, [current?.id]);

  useEffect(() => {
    // The selected file may still belong to the previous session during the
    // render before useWorkspace clears its state. Never add that stale file
    // back to the tabs after the session-reset effect has run.
    if (previousSessionId.current !== current?.id) {
      previousSessionId.current = current?.id;
      return;
    }
    if (!selectedFile?.path || closedPathsRef.current.has(selectedFile.path))
      return;
    setOpenTabs((tabs) =>
      tabs.includes(selectedFile.path) ? tabs : [...tabs, selectedFile.path],
    );
    setActivePath(selectedFile.path);
  }, [current?.id, selectedFile?.path]);

  const openFile = (path: string, line?: number) => {
    closedPathsRef.current.delete(path);
    onOpenFile(path, line);
  };

  const activateTab = (path: string) => {
    setActivePath(path);
    if (path !== selectedFile?.path) openFile(path);
  };

  const closeTab = (path: string) => {
    closedPathsRef.current.add(path);
    const remaining = openTabs.filter((item) => item !== path);
    setOpenTabs(remaining);
    if (activePath === path) {
      const next = remaining[remaining.length - 1];
      setActivePath(next);
      if (next) openFile(next);
      else {
        setMaximized(false);
        onCloseFile();
      }
    }
  };

  return (
    <aside className={`workspace${maximized ? " preview-maximized" : ""}`}>
      <div className="workspace-head">
        <div>
          <strong>Workspace</strong>
          {current && <small>{current.workspace}</small>}
        </div>
        <button className="icon-button" onClick={onRefresh} title="Refresh">
          <RefreshCw size={15} />
        </button>
      </div>
      <div className="tabs">
        <button
          className={tab === "files" ? "active" : ""}
          onClick={() => onSelectTab("files")}
        >
          <Folder size={14} /> Files
        </button>
        <button
          className={tab === "diff" ? "active" : ""}
          onClick={() => onSelectTab("diff")}
        >
          <GitCompare size={14} /> Diff
          {diff?.truncated && <span className="tab-warning">!</span>}
        </button>
      </div>
      {tab === "files" && (
        <>
          <div className="file-search">
            <Search size={14} />
            <input
              value={fileFilter}
              onChange={(event) => onFilterChange(event.target.value)}
              placeholder="Filter files"
            />
          </div>
          <FileTree
            key={current?.id}
            sessionId={current?.id}
            files={files}
            filter={fileFilter}
            onOpenFile={(path) => openFile(path)}
          />
        </>
      )}
      {tab === "diff" && <DiffViewer diff={diff} />}
      {openTabs.length > 0 && (
        <div className="workspace-preview-area">
          <div className="preview-tabs" role="tablist" aria-label="Open files">
            {openTabs.map((path) => (
              <div
                className={`preview-tab${path === activePath ? " active" : ""}`}
                key={path}
              >
                <button
                  role="tab"
                  aria-selected={path === activePath}
                  onClick={() => activateTab(path)}
                  title={path}
                >
                  {path.split("/").pop() || path}
                </button>
                <button
                  className="preview-tab-close"
                  onClick={() => closeTab(path)}
                  aria-label={`Close ${path}`}
                  title="Close tab"
                >
                  <X size={12} />
                </button>
              </div>
            ))}
          </div>
          {selectedFile?.path === activePath ? (
            <Preview
              file={selectedFile}
              line={selectedLine}
              onClose={() => closeTab(selectedFile.path)}
              onRetry={() => {
                onCloseFile();
                openFile(selectedFile.path);
              }}
              maximized={maximized}
              onToggleMaximize={() => setMaximized((value) => !value)}
            />
          ) : (
            <div className="preview-loading" role="status">
              Loading {activePath}…
            </div>
          )}
        </div>
      )}
    </aside>
  );
}
