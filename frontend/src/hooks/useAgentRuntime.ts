import { useCallback, useEffect, useState } from 'react';
import { api } from '../lib/api';
import type { Agent, NodeInfo } from '../types';

export function useAgentRuntime() {
  const [agents, setAgents] = useState<Agent[]>([]);
  const [node, setNode] = useState<NodeInfo>();
  const [nodeVersion, setNodeVersion] = useState('');

  const refresh = useCallback(async () => {
    const [catalog, nodeInfo] = await Promise.all([
      api<Agent[]>('/agent-catalog'),
      api<NodeInfo>('/node/versions'),
    ]);
    setAgents(catalog);
    setNode(nodeInfo);
    setNodeVersion(current => current || nodeInfo.active || '');
  }, []);

  useEffect(() => {
    refresh().catch(() => {});
  }, [refresh]);

  const nodeGroups = Object.entries(
    (node?.available || []).reduce((groups, version) => {
      const major = version.split('.')[0];
      (groups[major] ??= []).push(version);
      return groups;
    }, {} as Record<string, string[]>)
  ).sort(([a], [b]) => Number(b) - Number(a));

  return { agents, node, nodeVersion, setNodeVersion, nodeGroups, refresh };
}