import { useCallback, useEffect, useState } from 'react';
import { api } from '../lib/api';
import type { AgentModel } from '../types';

export function useAgentModels(agentId?: string) {
  const [models, setModels] = useState<AgentModel[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string>();

  const refresh = useCallback(async () => {
    if (!agentId) {
      setModels([]);
      return;
    }
    setLoading(true);
    try {
      setModels(await api<AgentModel[]>(`/agents/${agentId}/models`));
      setError(undefined);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      console.error('Failed to load agent models:', error);
      setModels([]);
      setError(message);
    } finally {
      setLoading(false);
    }
  }, [agentId]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  return { models, loading, error, refresh };
}
