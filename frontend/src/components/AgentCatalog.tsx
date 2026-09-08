import { CheckCircle2, Download, X } from 'lucide-react';
import type { Agent, NodeInfo } from '../types';

type Props = {
  open: boolean;
  agents: Agent[];
  node?: NodeInfo;
  nodeVersion: string;
  installingNode: boolean;
  installingAgent?: string;
  nodeGroups: [string, string[]][];
  onClose: () => void;
  onNodeVersionChange: (version: string) => void;
  onInstallNode: () => void;
  onInstallAgent: (id: string) => void;
};

export function AgentCatalog({ open, agents, node, nodeVersion, installingNode, installingAgent, nodeGroups, onClose, onNodeVersionChange, onInstallNode, onInstallAgent }: Props) {
  if (!open) return null;

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="agent-panel" onClick={event => event.stopPropagation()}>
        <div className="modal-head"><div><h3>Agents & runtimes</h3><p>Agent 不预装，Node.js 也不预装。</p></div><button className="icon-button" onClick={onClose}><X size={16} /></button></div>
        <section className="catalog-section">
          <div className="catalog-title"><strong>Node.js</strong><span>{node?.active || 'not installed'}</span></div>
          <div className="node-install">
            <select value={nodeVersion} onChange={event => onNodeVersionChange(event.target.value)}>
              <option value="">Select version</option>
              {nodeGroups.map(([major, versions]) => <optgroup key={major} label={`Node ${major}`}>{versions.map(version => <option key={version} value={version}>{version}</option>)}</optgroup>)}
            </select>
            <button className="primary-button" onClick={onInstallNode} disabled={!nodeVersion || installingNode}>{installingNode ? 'Installing…' : 'Install'}</button>
          </div>
        </section>
        <section className="catalog-section">
          <div className="catalog-title"><strong>Agents</strong><span>{agents.filter(agent => agent.installed).length} installed</span></div>
          <div className="agent-list">
            {agents.map(agent => <div className="agent-card" key={agent.id}>
              <div className="agent-info"><div className="agent-name">{agent.name} {agent.installed && <CheckCircle2 size={14} />}</div><p>{agent.description || 'No description'}</p>{agent.requirements.length > 0 && <small>Requires: {agent.requirements.join(', ')}</small>}</div>
              {agent.installed ? <span className="installed-label">Installed</span> : <button className="primary-button" onClick={() => onInstallAgent(agent.id)} disabled={installingAgent === agent.id}><Download size={14} /> {installingAgent === agent.id ? 'Installing…' : 'Install'}</button>}
            </div>)}
          </div>
        </section>
      </div>
    </div>
  );
}