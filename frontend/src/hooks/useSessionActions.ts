import { useCallback } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import { api } from '../lib/api';
import type { Session } from '../types';

type Options = {
  setSessions: Dispatch<SetStateAction<Session[]>>;
};

export function useSessionActions({ setSessions }: Options) {
  const createSession = useCallback(async (agentId: string, model?: string) => {
    const session = await api<Session>('/sessions', {
      method: 'POST',
      body: JSON.stringify({ agent_id: agentId, title: 'New Chat', model }),
    });
    setSessions(items => [session, ...items]);
    return session;
  }, [setSessions]);

  const setModel = useCallback(async (sessionId: string, model?: string) => {
    const session = await api<Session>(`/sessions/${sessionId}/model`, {
      method: 'PUT',
      body: JSON.stringify({ model: model || null }),
    });
    setSessions(items => items.map(item => item.id === sessionId ? session : item));
    return session;
  }, [setSessions]);

  const sendMessage = useCallback(async (sessionId: string, message: string) => {
    const result = await api<{ status?: string; error?: string }>(`/sessions/${sessionId}/messages`, {
      method: 'POST',
      body: JSON.stringify({ message }),
    });
    if (result.error) throw new Error(result.error);
    setSessions(items => items.map(item => item.id === sessionId ? { ...item, status: 'running' } : item));
  }, [setSessions]);

  const deleteSession = useCallback(async (sessionId: string) => {
    await api(`/sessions/${sessionId}`, { method: 'DELETE' });
    setSessions(items => items.filter(item => item.id !== sessionId));
  }, [setSessions]);

  const interrupt = useCallback(async (sessionId: string) => {
    const result = await api<{ status?: string }>(`/sessions/${sessionId}/interrupt`, { method: 'POST' });
    setSessions(items => items.map(item => item.id === sessionId
      ? { ...item, status: result.status || 'interrupted' }
      : item));
  }, [setSessions]);

  return { createSession, setModel, sendMessage, deleteSession, interrupt };
}
