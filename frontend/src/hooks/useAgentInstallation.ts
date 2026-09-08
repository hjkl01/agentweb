import { useState } from 'react';
import { api } from '../lib/api';
import type { Agent } from '../types';

export function useAgentInstallation(agents: Agent[], nodeVersion: string, node?: { installed: string[] }, refreshAgents?: () => Promise<void>) {
  const [installingNode, setInstallingNode] = useState(false);
  const [installingAgent, setInstallingAgent] = useState<string>();
  const [error, setError] = useState<string>();

  const installNode = async () => {
    const version = nodeVersion.trim();
    if (!version) return;
    setInstallingNode(true);
    setError(undefined);
    try {
      await api('/node/install', { method: 'POST', body: JSON.stringify({ version }) });
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
    if (!agent?.install_command) return;
    if (agent.requirements.includes('Node.js') && !(node?.installed || []).includes(nodeVersion)) {
      const message = '请先安装 Node.js；安装后再点击 Agent 的安装按钮。';
      setError(message);
      throw new Error(message);
    }
    setInstallingAgent(id);
    setError(undefined);
    try {
      await api('/agents/custom/install', { method: 'POST', body: JSON.stringify({ command: agent.install_command }) });
      await refreshAgents?.();
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setError(message);
      throw new Error(message);
    } finally {
      setInstallingAgent(undefined);
    }
  };

  const installCustomAgent = async (command: string) => {
    const value = command.trim();
    if (!value) return;
    setInstallingAgent('custom');
    setError(undefined);
    try {
      await api('/agents/custom/install', { method: 'POST', body: JSON.stringify({ command: value }) });
      await refreshAgents?.();
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setError(message);
      throw new Error(message);
    } finally {
      setInstallingAgent(undefined);
    }
  };

  return { installingNode, installingAgent, error, installNode, installAgent, installCustomAgent };
}
