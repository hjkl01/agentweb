import { useCallback, useEffect, useState } from 'react';
import { api } from '../lib/api';
import type { ActivityItem, AgentEvent, ChatMessage, FileItem, Session } from '../types';

export function useSession(active?: string) {
  const [sessions, setSessions] = useState<Session[]>([]);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [files, setFiles] = useState<FileItem[]>([]);
  const [stream, setStream] = useState('');
  const [activity, setActivity] = useState<ActivityItem[]>([]);
  const [activityOpen, setActivityOpen] = useState(false);

  const refreshSessions = useCallback(async () => {
    setSessions(await api<Session[]>('/sessions'));
  }, []);

  useEffect(() => {
    refreshSessions().catch(() => {});
  }, [refreshSessions]);

  useEffect(() => {
    if (!active) return;
    setStream('');
    setActivity([]);
    setActivityOpen(false);

    Promise.all([
      api<ChatMessage[]>(`/sessions/${active}/messages`),
      api<FileItem[]>(`/sessions/${active}/files`),
    ]).then(([loadedMessages, loadedFiles]) => {
      setMessages(loadedMessages);
      setFiles(loadedFiles);
    }).catch(() => {});

    const protocol = location.protocol === 'https:' ? 'wss' : 'ws';
    const ws = new WebSocket(`${protocol}://${location.host}/api/sessions/${active}/events`);

    ws.onmessage = event => {
      const ev = JSON.parse(event.data) as AgentEvent;
      const data = ev.data || {};
      const addActivity = (label: string, detail?: string) => {
        setActivity(items => [...items, { id: crypto.randomUUID(), type: ev.type, label, detail }].slice(-40));
        setActivityOpen(true);
      };

      if (ev.type === 'message.started') addActivity('Agent started');
      if (ev.type === 'message.delta') setStream(value => value + (data.text || ''));
      if (ev.type === 'message.completed') {
        setStream(value => {
          if (value) setMessages(items => [...items, { id: crypto.randomUUID(), role: 'assistant', content: value }]);
          return '';
        });
      }
      if (ev.type === 'thinking.started') addActivity('Thinking');
      if (ev.type === 'thinking.delta') addActivity('Thinking', data.text);
      if (ev.type === 'thinking.completed') addActivity('Thinking completed');
      if (ev.type === 'tool.started') addActivity(`Tool: ${data.tool}`);
      if (ev.type === 'tool.output') addActivity(`Tool output: ${data.tool}`, data.output);
      if (ev.type === 'tool.completed') addActivity(`Tool completed: ${data.tool}`);
      if (ev.type === 'command.started') addActivity(`Command: ${data.command}`);
      if (ev.type === 'command.output') addActivity('Command output', data.output);
      if (ev.type === 'command.completed') addActivity('Command completed');
      if (ev.type === 'file.created') addActivity('File created', data.path);
      if (ev.type === 'file.modified') addActivity('File modified', data.path);
      if (ev.type === 'file.deleted') addActivity('File deleted', data.path);
      if (ev.type === 'agent.error' || ev.type === 'error') addActivity('Agent error', data.message);

      if (ev.type === 'session.completed' || ev.type === 'session.error') {
        api<ChatMessage[]>(`/sessions/${active}/messages`).then(setMessages).catch(() => {});
        api<FileItem[]>(`/sessions/${active}/files`).then(setFiles).catch(() => {});
        refreshSessions().catch(() => {});
      }
    };

    return () => ws.close();
  }, [active, refreshSessions]);

  const createSession = useCallback(async (agentId: string) => {
    const session = await api<Session>('/sessions', {
      method: 'POST',
      body: JSON.stringify({ agent_id: agentId, workspace: '/workspaces', title: 'New Chat' }),
    });
    setSessions(items => [session, ...items]);
    return session;
  }, []);

  const sendMessage = useCallback(async (sessionId: string, message: string) => {
    await api(`/sessions/${sessionId}/messages`, {
      method: 'POST',
      body: JSON.stringify({ message }),
    });
    setSessions(items => items.map(item => item.id === sessionId ? { ...item, status: 'running' } : item));
  }, []);

  const deleteSession = useCallback(async (sessionId: string) => {
    await api(`/sessions/${sessionId}`, { method: 'DELETE' });
    setSessions(items => items.filter(item => item.id !== sessionId));
  }, []);

  return {
    sessions, setSessions, messages, setMessages, files, setFiles, stream,
    activity, activityOpen, setActivityOpen, refreshSessions,
    createSession, sendMessage, deleteSession,
  };
}