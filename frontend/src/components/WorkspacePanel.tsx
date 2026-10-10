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
import { useEffect, useState } from "react";
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
  maximized,
  onToggleMaximize,
}: {
  file: WorkspaceFile;
  line?: number;
  onClose: () => void;
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
          <button className="icon-button" onClick={onClose} title="Close preview" aria-label="Close preview">
            <X size={15} />
          </button>
        </div>
      </div>
      <PreviewNotice file={file} />
      {!unavailable && <CodePreview file={file} focusLine={line} />}
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

  useEffect(() => {
    setOpenTabs([]);
    setActivePath(undefined);
    setMaximized(false);
  }, [current?.id]);

  useEffect(() => {
    if (!selectedFile?.path) return;
    setOpenTabs((tabs) => tabs.includes(selectedFile.path) ? tabs : [...tabs, selectedFile.path]);
    setActivePath(selectedFile.path);
  }, [selectedFile?.path]);

  const activateTab = (path: string) => {
    setActivePath(path);
    if (path !== selectedFile?.path) onOpenFile(path);
  };

  const closeTab = (path: string) => {
    const remaining = openTabs.filter((item) => item !== path);
    setOpenTabs(remaining);
    if (activePath === path) {
      const next = remaining[remaining.length - 1];
      setActivePath(next);
      if (next) onOpenFile(next);
      else onCloseFile();
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
            files={files}
            filter={fileFilter}
            onOpenFile={(path) => onOpenFile(path)}
          />
        </>
      )}
      {tab === "diff" && <DiffViewer diff={diff} />}
      {selectedFile && (
        <div className="workspace-preview-area">
          <div className="preview-tabs" role="tablist" aria-label="Open files">
            {openTabs.map((path) => (
              <div className={`preview-tab${path === activePath ? " active" : ""}`} key={path}>
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
          <Preview
            file={selectedFile}
            line={selectedLine}
            onClose={() => closeTab(selectedFile.path)}
            maximized={maximized}
            onToggleMaximize={() => setMaximized((value) => !value)}
          />
        </div>
      )}
    </aside>
  );
}
