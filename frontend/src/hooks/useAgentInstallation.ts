import { useState } from 'react';
import { api } from '../lib/api';
import type { Agent, NodeInfo } from '../types';

export function useAgentInstallation(
  agents: Agent[],
  nodeVersion: string,
  node?: NodeInfo,
  refreshAgents?: () => Promise<void>,
) {
  const [installingNode, setInstallingNode] = useState(false);
  const [installingAgent, setInstallingAgent] = useState<string>();
  const [error, setError] = useState<string>();

  const installNode = async () => {
    if (!nodeVersion) return;
    setInstallingNode(true);
    setError(undefined);
    try {
      await api('/node/install', {
        method: 'POST',
        body: JSON.stringify({ version: nodeVersion }),
      });
      await refreshAgents?.();
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setError(message);
      throw new Error(message);
    } finally {
      setInstallingNode(false);
    }
  };

  const installAgent = async (id: string) => {
    const agent = agents.find(item => item.id === id);
    if (agent?.requirements.includes('Node.js') && !node?.installed.includes(nodeVersion)) {
      const message = '请先选择并安装一个 Node.js 版本';
      setError(message);
      throw new Error(message);
    }
    setInstallingAgent(id);
    setError(undefined);
    try {
      await api(`/agents/${id}/install`, { method: 'POST' });
      // Installation is asynchronous on the backend. Keep the button disabled
      // briefly through the current request, then refresh the catalog so a
      // completed installation is reflected when the backend reports it.
      await refreshAgents?.();
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setError(message);
      throw new Error(message);
    } finally {
      setInstallingAgent(undefined);
    }
  };

  return { installingNode, installingAgent, error, installNode, installAgent };
}
