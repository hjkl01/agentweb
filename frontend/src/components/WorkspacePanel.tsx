import { AlertTriangle, FileWarning, Folder, GitCompare, RefreshCw, Search, X } from 'lucide-react';
import { CodePreview } from './CodePreview';
import { DiffViewer } from './DiffViewer';
import { FileTree } from './FileTree';
import type { FileItem, Session, WorkspaceDiff, WorkspaceFile } from '../types';

type Props = {
  current?: Session; files: FileItem[]; fileFilter: string; selectedFile?: WorkspaceFile; selectedLine?: number; diff?: WorkspaceDiff; tab: 'files' | 'diff';
  onFilterChange: (value: string) => void; onSelectTab: (tab: 'files' | 'diff') => void; onRefresh: () => void; onOpenFile: (path: string, line?: number) => void; onCloseFile: () => void;
};

function PreviewNotice({ file }: { file: WorkspaceFile }) {
  if (!file.binary && !file.truncated) return null;
  return <div className="preview-notice"><span>{file.binary ? <FileWarning size={15} /> : <AlertTriangle size={15} />}</span><span>{file.message || (file.binary ? 'Binary file preview is not supported.' : 'File preview was truncated.')}</span></div>;
}

export function WorkspacePanel({ current, files, fileFilter, selectedFile, selectedLine, diff, tab, onFilterChange, onSelectTab, onRefresh, onOpenFile, onCloseFile }: Props) {
  return <aside className="workspace">
    <div className="workspace-head"><div><strong>Workspace</strong>{current && <small>{current.workspace}</small>}</div><button className="icon-button" onClick={onRefresh} title="Refresh"><RefreshCw size={15} /></button></div>
    <div className="tabs"><button className={tab === 'files' ? 'active' : ''} onClick={() => onSelectTab('files')}><Folder size={14} /> Files</button><button className={tab === 'diff' ? 'active' : ''} onClick={() => onSelectTab('diff')}><GitCompare size={14} /> Diff{diff?.truncated && <span className="tab-warning">!</span>}</button></div>
    {tab === 'files' && <><div className="file-search"><Search size={14} /><input value={fileFilter} onChange={event => onFilterChange(event.target.value)} placeholder="Filter files" /></div><FileTree files={files} filter={fileFilter} onOpenFile={path => onOpenFile(path)} /></>}
    {tab === 'diff' && <DiffViewer diff={diff} onOpenFile={(path, line) => { onSelectTab('files'); onOpenFile(path, line); }} />}
    {selectedFile && <div className="preview"><div className="preview-head"><div><strong>{selectedFile.path}</strong><small>File preview{selectedLine ? ` · line ${selectedLine}` : ''}{selectedFile.size !== undefined ? ` · ${selectedFile.size.toLocaleString()} bytes` : ''}</small></div><button className="icon-button" onClick={onCloseFile}><X size={15} /></button></div><PreviewNotice file={selectedFile} />{!selectedFile.binary && !selectedFile.truncated && <CodePreview file={selectedFile} focusLine={selectedLine} />}</div>}
  </aside>;
}
