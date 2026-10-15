import { useCallback, useEffect, useRef, useState } from "react";
import { api, fileUrl } from "../lib/api";
import type { FileItem, WorkspaceDiff, WorkspaceFile } from "../types";

export type WorkspaceTab = "files" | "diff";

export function useWorkspace(active?: string, revision = 0) {
  const [files, setFiles] = useState<FileItem[]>([]);
  const [selectedFile, setSelectedFile] = useState<WorkspaceFile>();
  const [diff, setDiff] = useState<WorkspaceDiff>();
  const [tab, setTab] = useState<WorkspaceTab>("files");
  const [filter, setFilter] = useState("");
  const [error, setError] = useState<string>();
  const activeRef = useRef(active);
  const filesRequest = useRef(0);
  const diffRequest = useRef(0);
  const selectedFileRequest = useRef(0);
  activeRef.current = active;

  const refreshFiles = useCallback(async () => {
    const request = ++filesRequest.current;
    const requestActive = active;
    if (!requestActive) {
      setFiles([]);
      return;
    }
    try {
      const result = await api<FileItem[]>(`/sessions/${requestActive}/files`);
      if (request !== filesRequest.current || activeRef.current !== requestActive) return;
      setFiles(result);
      setError(undefined);
    } catch (error) {
      if (request !== filesRequest.current || activeRef.current !== requestActive) return;
      setError(error instanceof Error ? error.message : String(error));
    }
  }, [active]);

  const refreshDiff = useCallback(async () => {
    const request = ++diffRequest.current;
    const requestActive = active;
    if (!requestActive) {
      setDiff(undefined);
      return;
    }
    try {
      const result = await api<WorkspaceDiff>(`/sessions/${requestActive}/diff`);
      if (request !== diffRequest.current || activeRef.current !== requestActive) return;
      setDiff(result);
      setError(undefined);
    } catch (error) {
      if (request !== diffRequest.current || activeRef.current !== requestActive) return;
      const message = error instanceof Error ? error.message : String(error);
      setDiff({ diff: message });
      setError(message);
    }
  }, [active]);

  const refresh = useCallback(async () => {
    const requestActive = active;
    await refreshFiles();
    if (activeRef.current !== requestActive) return;
    if (tab === "diff") await refreshDiff();
  }, [active, refreshFiles, refreshDiff, tab]);

  useEffect(() => {
    // Invalidate in-flight work before a new session starts loading.
    filesRequest.current += 1;
    diffRequest.current += 1;
    selectedFileRequest.current += 1;
    setFiles([]);
    setSelectedFile(undefined);
    setDiff(undefined);
    setFilter("");
    setTab("files");
    setError(undefined);
    refreshFiles().catch(console.error);
  }, [active, refreshFiles]);

  const openFile = useCallback(
    async (path: string) => {
      const request = ++selectedFileRequest.current;
      const requestActive = active;
      if (!requestActive) return;
      try {
        const result = await api<WorkspaceFile>(fileUrl(requestActive, path));
        if (request !== selectedFileRequest.current || activeRef.current !== requestActive) return;
        setSelectedFile(result);
        setError(undefined);
      } catch (error) {
        if (request !== selectedFileRequest.current || activeRef.current !== requestActive) return;
        const message = error instanceof Error ? error.message : String(error);
        setSelectedFile({ path, content: "", error: message });
        setError(message);
      }
    },
    [active],
  );

  useEffect(() => {
    if (revision <= 0 || !active) return;
    refresh().catch(console.error);
    const path = selectedFile?.path;
    if (!path) return;
    openFile(path).catch(console.error);
  }, [active, revision, refresh, selectedFile?.path, openFile]);

  const selectTab = useCallback(
    (nextTab: WorkspaceTab) => {
      setTab(nextTab);
      if (nextTab === "diff") refreshDiff().catch(console.error);
    },
    [refreshDiff],
  );

  return {
    files,
    selectedFile,
    diff,
    tab,
    filter,
    error,
    setFilter,
    setSelectedFile,
    selectTab,
    openFile,
    refresh,
  };
}
