import { useCallback, useEffect, useState } from 'react';
import { api, fileUrl } from '../lib/api';
import type { FileItem, WorkspaceDiff, WorkspaceFile } from '../types';

export type WorkspaceTab = 'files' | 'diff';

export function useWorkspace(active?: string) {
  const [files, setFiles] = useState<FileItem[]>([]);
  const [selectedFile, setSelectedFile] = useState<WorkspaceFile>();
  const [diff, setDiff] = useState<WorkspaceDiff>();
  const [tab, setTab] = useState<WorkspaceTab>('files');
  const [filter, setFilter] = useState('');
  const [error, setError] = useState<string>();

  const refreshFiles = useCallback(async () => {
    if (!active) {
      setFiles([]);
      return;
    }
    try {
      setFiles(await api<FileItem[]>(`/sessions/${active}/files`));
      setError(undefined);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      console.error('Failed to load workspace files:', error);
      setError(message);
    }
  }, [active]);

  const refreshDiff = useCallback(async () => {
    if (!active) {
      setDiff(undefined);
      return;
    }
    try {
      setDiff(await api<WorkspaceDiff>(`/sessions/${active}/diff`));
      setError(undefined);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      console.error('Failed to load workspace diff:', error);
      setDiff({ diff: message });
      setError(message);
    }
  }, [active]);

  const refresh = useCallback(async () => {
    await refreshFiles();
    if (tab === 'diff') await refreshDiff();
  }, [refreshFiles, refreshDiff, tab]);

  useEffect(() => {
    setSelectedFile(undefined);
    setFilter('');
    setTab('files');
    refreshFiles();
  }, [active, refreshFiles]);

  const openFile = useCallback(async (path: string) => {
    if (!active) return;
    try {
      setSelectedFile(await api<WorkspaceFile>(fileUrl(active, path)));
      setError(undefined);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setSelectedFile({ path, content: message });
      setError(message);
    }
  }, [active]);

  const selectTab = useCallback((nextTab: WorkspaceTab) => {
    setTab(nextTab);
    if (nextTab === 'diff') refreshDiff();
  }, [refreshDiff]);

  return {
    files, selectedFile, diff, tab, filter, error,
    setFilter, setSelectedFile, selectTab, openFile, refresh,
  };
}
