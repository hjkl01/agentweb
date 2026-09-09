import { useState } from 'react';
import { api } from '../lib/api';
import type { Agent, NodeInfo } from '../types';

type InstallFeedback = { version: string; state: 'success' | 'error'; message: string };

export function useAgentInstallation(agents: Agent[], nodeVersion: string, node?: NodeInfo, refreshAgents?: () => Promise<void>) {
  const [installingNode, setInstallingNode] = useState(false);
  const [installingAgent, setInstallingAgent] = useState<string>();
  const [error, setError] = useState<string>();
  const [nodeInstallFeedback, setNodeInstallFeedback] = useState<InstallFeedback>();

  const installNode = async (requestedVersion?: string) => {
    const version = (requestedVersion || nodeVersion).trim();
    if (!version || installingNode) return;
    setInstallingNode(true);
    setError(undefined);
    setNodeInstallFeedback(undefined);
    try {
      const result = await api<{ status:string; version:string }>('/node/install', { method: 'POST', body: JSON.stringify({ version }) });
      const installedVersion = result.version || version;
      setNodeInstallFeedback({ version: installedVersion, state: 'success', message: `Node.js ${installedVersion} 安装成功` });
      await refreshAgents?.();
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setError(message);
      setNodeInstallFeedback({ version, state: 'error', message: `Node.js ${version} 安装失败：${message}` });
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

  return { installingNode, installingAgent, error, nodeInstallFeedback, installNode, installAgent, installCustomAgent };
}
