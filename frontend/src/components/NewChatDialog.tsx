import { Bot, ChevronRight, X } from 'lucide-react';
import type { Agent } from '../types';

type Props = {
  open: boolean;
  agents: Agent[];
  onClose: () => void;
  onSelectAgent: (id: string) => void;
  onManageAgents: () => void;
};

export function NewChatDialog({ open, agents, onClose, onSelectAgent, onManageAgents }: Props) {
  if (!open) return null;
  const installedAgents = agents.filter(agent => agent.installed);

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={event => event.stopPropagation()}>
        <div className="modal-head">
          <div><h3>New chat</h3><p>选择要运行的 Agent。</p></div>
          <button className="icon-button" onClick={onClose}><X size={16} /></button>
        </div>
        {installedAgents.length ? (
          <div className="agent-choice-list">
            {installedAgents.map(agent => (
              <button className="agent-choice" key={agent.id} onClick={() => onSelectAgent(agent.id)}>
                <div className="agent-choice-icon"><Bot size={17} /></div>
                <div><strong>{agent.name}</strong><span>{agent.description || 'Ready to use'}</span></div>
                <ChevronRight size={16} />
              </button>
            ))}
          </div>
        ) : (
          <div className="modal-empty">
            <Bot size={24} />
            <strong>No Agent installed</strong>
            <p>先到 Agents & runtimes 安装一个 Agent。</p>
            <button className="primary-button" onClick={onManageAgents}>Manage agents</button>
          </div>
        )}
      </div>
    </div>
  );
}