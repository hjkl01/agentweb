import { useEffect } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import { api } from '../lib/api';
import { applyAgentEvent } from '../lib/agentActivity';
import type { ActivityItem, AgentEvent, ChatMessage, FileItem } from '../types';

type Options = {
  active?: string;
  refreshSessions: () => Promise<void>;
  setMessages: Dispatch<SetStateAction<ChatMessage[]>>;
  setFiles: Dispatch<SetStateAction<FileItem[]>>;
  setStream: Dispatch<SetStateAction<string>>;
  setActivity: Dispatch<SetStateAction<ActivityItem[]>>;
  setActivityOpen: Dispatch<SetStateAction<boolean>>;
  setWorkspaceRevision: Dispatch<SetStateAction<number>>;
  setError: (message?: string) => void;
};

export function useSessionEvents(options: Options) {
  const { active } = options;
  useEffect(() => {
    if (!active) return;
    options.setStream(''); options.setActivity([]); options.setActivityOpen(false);
    Promise.all([api<ChatMessage[]>(`/sessions/${active}/messages`), api<FileItem[]>(`/sessions/${active}/files`)]).then(([messages, files]) => {
      options.setMessages(messages); options.setFiles(files); options.setError(undefined);
    }).catch(error => options.setError(error instanceof Error ? error.message : String(error)));

    const protocol = location.protocol === 'https:' ? 'wss' : 'ws';
    const ws = new WebSocket(`${protocol}://${location.host}/api/sessions/${active}/events`);
    ws.onerror = () => options.setError('WebSocket 连接失败，请确认后端正在运行。');
    ws.onmessage = event => handleEvent(event.data, active, options);
    return () => ws.close();
  }, [active]);
}

function handleEvent(raw: string, active: string, options: Options) {
  let event: AgentEvent;
  try { event = JSON.parse(raw) as AgentEvent; } catch { return; }
  const data = event.data || {};
  if (event.type === 'message.delta') options.setStream(value => value + (data.text || ''));
  if (event.type === 'message.completed') options.setStream(value => {
    if (value) options.setMessages(items => [...items, { id: crypto.randomUUID(), role: 'assistant', content: value }]);
    return '';
  });

  const activityEvent = event.type.startsWith('thinking.') || event.type.startsWith('tool.') || event.type.startsWith('command.') || event.type.startsWith('file.') || event.type === 'message.started' || event.type === 'agent.error' || event.type === 'error';
  if (activityEvent) { options.setActivity(items => applyAgentEvent(items, event)); options.setActivityOpen(true); }
  if (event.type.startsWith('file.')) {
    options.setWorkspaceRevision(value => value + 1);
    api<FileItem[]>(`/sessions/${active}/files`).then(options.setFiles).catch(console.error);
  }
  if (event.type === 'session.completed' || event.type === 'session.error') {
    api<ChatMessage[]>(`/sessions/${active}/messages`).then(options.setMessages).catch(console.error);
    api<FileItem[]>(`/sessions/${active}/files`).then(options.setFiles).catch(console.error);
    options.refreshSessions().catch(console.error);
  }
}
