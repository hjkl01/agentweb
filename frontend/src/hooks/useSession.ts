import { useCallback, useEffect, useState } from 'react';
import { api } from '../lib/api';
import type { ActivityItem, ChatMessage, FileItem, Session } from '../types';
import { useSessionActions } from './useSessionActions';
import { useSessionEvents } from './useSessionEvents';

export function useSession(active?: string) {
  const [sessions, setSessions] = useState<Session[]>([]);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [files, setFiles] = useState<FileItem[]>([]);
  const [stream, setStream] = useState('');
  const [activity, setActivity] = useState<ActivityItem[]>([]);
  const [activityOpen, setActivityOpen] = useState(false);
  const [workspaceRevision, setWorkspaceRevision] = useState(0);
  const [error, setError] = useState<string>();

  const refreshSessions = useCallback(async () => {
    try { setSessions(await api<Session[]>('/sessions')); setError(undefined); }
    catch (error) { setError(error instanceof Error ? error.message : String(error)); }
  }, []);

  useEffect(() => { refreshSessions().catch(console.error); }, [refreshSessions]);
  const actions = useSessionActions({ setSessions });
  useSessionEvents({ active, refreshSessions, setMessages, setFiles, setStream, setActivity, setActivityOpen, setWorkspaceRevision, setError });

  useEffect(() => {
    if (!active) { setMessages([]); setFiles([]); setStream(''); setActivity([]); setActivityOpen(false); setWorkspaceRevision(0); }
  }, [active]);

  return { sessions, messages, stream, activity, activityOpen, workspaceRevision, error, setMessages, setFiles, setActivity, setActivityOpen, refreshSessions, ...actions };
}
