import { useEffect, useRef, useState } from 'react';
import { api } from '../lib/api';
import type { Agent, NodeInfo } from '../types';

type InstallFeedback = { version: string; state: 'success' | 'error'; message: string };
type AgentInstallFeedback = { agentId: string; state: 'success' | 'error' | 'running'; message: string };

type InstallEvent = {
  type: 'install.output' | 'install.completed';
  data: { agent_id: string; text?: string; success?: boolean };
};

const appendInstallOutput = (current: AgentInstallFeedback | undefined, agentId: string, text: string) => {
  if (!text) return current;
  const previous = current?.agentId === agentId ? current.message : '';
  const output = [previous, text].filter(Boolean).join('\n');
  // Keep the UI useful even when npm emits a very large log.
  const trimmed = output.length > 6000 ? output.slice(-6000) : output;
  return { agentId, state: 'running' as const, message: trimmed };
};

export function useAgentInstallation(agents: Agent[], nodeVersion: string, node?: NodeInfo, refreshAgents?: () => Promise<void>) {
  const [installingNode, setInstallingNode] = useState(false);
  const [installingAgent, setInstallingAgent] = useState<string>();
  const [error, setError] = useState<string>();
  const [nodeInstallFeedback, setNodeInstallFeedback] = useState<InstallFeedback>();
  const [agentInstallFeedback, setAgentInstallFeedback] = useState<AgentInstallFeedback>();
  const installTimeout = useRef<ReturnType<typeof setTimeout>>();

  useEffect(() => {
    let disposed = false;
    let ws: WebSocket | undefined;
    let reconnectTimer: ReturnType<typeof setTimeout> | undefined;
    let delay = 500;

    const connect = () => {
      if (disposed) return;
      const protocol = location.protocol === 'https:' ? 'wss' : 'ws';
      ws = new WebSocket(`${protocol}://${location.host}/api/events`);
      ws.onopen = () => { delay = 500; };
      ws.onmessage = event => {
        let value: InstallEvent;
        try { value = JSON.parse(event.data) as InstallEvent; } catch { return; }
        const data = value.data || {};
        if (!data.agent_id) return;
        if (value.type === 'install.output') {
          setAgentInstallFeedback(current => appendInstallOutput(current, data.agent_id, data.text || ''));
          return;
        }
        if (value.type === 'install.completed') {
          if (installTimeout.current) clearTimeout(installTimeout.current);
          setInstallingAgent(undefined);
          if (data.success) {
            setAgentInstallFeedback(current => ({
              agentId: data.agent_id,
              state: 'success',
              message: current?.agentId === data.agent_id && current.message
                ? `安装成功\n\n${current.message}`
                : `${data.agent_id} 安装成功`,
            }));
            setError(undefined);
          } else {
            setAgentInstallFeedback(current => ({
              agentId: data.agent_id,
              state: 'error',
              message: current?.agentId === data.agent_id && current.message
                ? `安装失败，后端返回的详细错误：\n\n${current.message}`
                : '安装失败，但没有收到后端的详细错误输出。',
            }));
            setError(current => current || `Agent ${data.agent_id} 安装失败`);
          }
          refreshAgents?.().catch(console.error);
        }
      };
      ws.onerror = () => ws?.close();
      ws.onclose = () => {
        if (disposed) return;
        reconnectTimer = setTimeout(connect, delay);
        delay = Math.min(delay * 2, 8000);
      };
    };
    connect();
    return () => {
      disposed = true;
      if (reconnectTimer) clearTimeout(reconnectTimer);
      if (installTimeout.current) clearTimeout(installTimeout.current);
      ws?.close();
    };
  }, [refreshAgents]);

  const installNode = async (requestedVersion?: string) => {
    const version = (requestedVersion || nodeVersion).trim();
    if (!version || installingNode || installingAgent) return;
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
    if (installingAgent || installingNode) return;
    const agent = agents.find(item => item.id === id);
    if (!agent?.install_command) return;
    if (agent.requirements.includes('Node.js') && !(node?.installed || []).length) {
      const message = '请先安装 Node.js；安装后再点击 Agent 的安装按钮。';
      setError(message);
      throw new Error(message);
    }
    setInstallingAgent(id);
    setError(undefined);
    setAgentInstallFeedback({ agentId: id, state: 'running', message: `正在安装 ${agent.name}…` });
    try {
      await api(`/agents/${encodeURIComponent(id)}/install`, { method: 'POST' });
      // The backend installation is asynchronous. Do not clear installingAgent here;
      // install.completed is the authoritative success/failure signal.
      installTimeout.current = setTimeout(() => {
        setInstallingAgent(current => current === id ? undefined : current);
        setAgentInstallFeedback(current => current?.agentId === id && current.state === 'running'
          ? { ...current, state: 'error', message: `安装超时。最近的后端输出：\n\n${current.message}` }
          : current);
      }, 5 * 60 * 1000);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setError(message);
      setAgentInstallFeedback({ agentId: id, state: 'error', message: `安装请求失败：${message}` });
      setInstallingAgent(undefined);
      throw new Error(message);
    }
  };

  const installCustomAgent = async (command: string) => {
    const value = command.trim();
    if (!value || installingAgent || installingNode) return;
    setInstallingAgent('custom');
    setError(undefined);
    try {
      const result = await api<{ output?: string }>('/agents/custom/install', { method: 'POST', body: JSON.stringify({ command: value }) });
      setAgentInstallFeedback({ agentId: 'custom', state: 'success', message: result.output ? `安装成功：${result.output}` : '自定义 Agent 安装成功' });
      await refreshAgents?.();
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setError(message);
      setAgentInstallFeedback({ agentId: 'custom', state: 'error', message: `安装失败：${message}` });
      throw new Error(message);
    } finally {
      setInstallingAgent(undefined);
    }
  };

  return { installingNode, installingAgent, error, nodeInstallFeedback, agentInstallFeedback, installNode, installAgent, installCustomAgent };
}
