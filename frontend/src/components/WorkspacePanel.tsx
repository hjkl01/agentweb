import { File, Folder, GitCompare, RefreshCw, Search, X } from 'lucide-react';
import type { FileItem, Session, WorkspaceFile } from '../types';

type Props = {
  current?: Session;
  files: FileItem[];
  fileFilter: string;
  selectedFile?: WorkspaceFile;
  diff?: any;
  tab: 'files' | 'diff';
  onFilterChange: (value: string) => void;
  onSelectTab: (tab: 'files' | 'diff') => void;
  onRefresh: () => void;
  onOpenFile: (path: string) => void;
  onCloseFile: () => void;
};

export function WorkspacePanel({ current, files, fileFilter, selectedFile, diff, tab, onFilterChange, onSelectTab, onRefresh, onOpenFile, onCloseFile }: Props) {
  const filteredFiles = files.filter(file => file.path.toLowerCase().includes(fileFilter.toLowerCase()));

  return (
    <aside className="workspace">
      <div className="workspace-head"><div><strong>Workspace</strong>{current && <small>{current.workspace}</small>}</div><button className="icon-button" onClick={onRefresh} title="Refresh"><RefreshCw size={15} /></button></div>
      <div className="tabs">
        <button className={tab === 'files' ? 'active' : ''} onClick={() => onSelectTab('files')}><Folder size={14} /> Files</button>
        <button className={tab === 'diff' ? 'active' : ''} onClick={() => onSelectTab('diff')}><GitCompare size={14} /> Diff</button>
      </div>
      {tab === 'files' && <>
        <div className="file-search"><Search size={14} /><input value={fileFilter} onChange={event => onFilterChange(event.target.value)} placeholder="Filter files" /></div>
        <div className="filetree">
          {filteredFiles.map(file => <button className="file" key={file.path} onClick={() => onOpenFile(file.path)}><File size={14} /><span>{file.path}</span></button>)}
          {!filteredFiles.length && <div className="workspace-empty">No files</div>}
        </div>
      </>}
      {tab === 'diff' && <pre className="diff">{diff?.status}{'\n'}{diff?.diff || 'No git diff'}</pre>}
      {selectedFile && <div className="preview">
        <div className="preview-head"><div><strong>{selectedFile.path}</strong><small>File preview</small></div><button className="icon-button" onClick={onCloseFile}><X size={15} /></button></div>
        <pre>{selectedFile.content}</pre>
      </div>}
    </aside>
  );
}