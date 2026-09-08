import { Bot, ChevronLeft, ChevronRight, Loader2, X } from 'lucide-react';
import { useEffect, useState } from 'react';
import { useAgentModels } from '../hooks/useAgentModels';
import type { Agent } from '../types';

type Props = {
  open: boolean;
  agents: Agent[];
  onClose: () => void;
  onSelectAgent: (id: string, model?: string) => void;
  onManageAgents: () => void;
};

export function NewChatDialog({ open, agents, onClose, onSelectAgent, onManageAgents }: Props) {
  const [selectedAgentId, setSelectedAgentId] = useState<string>();
  const [selectedModel, setSelectedModel] = useState<string>();
  const installedAgents = agents.filter(agent => agent.installed);
  const models = useAgentModels(selectedAgentId);

  useEffect(() => {
    if (!open) {
      setSelectedAgentId(undefined);
      setSelectedModel(undefined);
    }
  }, [open]);

  useEffect(() => {
    setSelectedModel(models.models[0]?.id);
  }, [selectedAgentId, models.models]);

  if (!open) return null;
  const selectedAgent = installedAgents.find(agent => agent.id === selectedAgentId);

  const chooseAgent = (id: string) => {
    setSelectedAgentId(id);
    setSelectedModel(undefined);
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal new-chat-modal" onClick={event => event.stopPropagation()}>
        <div className="modal-head">
          <div><h3>New chat</h3><p>{selectedAgent ? `配置 ${selectedAgent.name} 的模型。` : '选择要运行的 Agent。'}</p></div>
          <button className="icon-button" onClick={onClose}><X size={16} /></button>
        </div>

        {!selectedAgent ? (
          installedAgents.length ? (
            <div className="agent-choice-list">
              {installedAgents.map(agent => (
                <button className="agent-choice" key={agent.id} onClick={() => chooseAgent(agent.id)}>
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
          )
        ) : (
          <div className="model-picker">
            <button className="back-button" onClick={() => chooseAgent('')}><ChevronLeft size={15} /> 更换 Agent</button>
            <div className="selected-agent"><div className="agent-choice-icon"><Bot size={17} /></div><strong>{selectedAgent.name}</strong></div>
            <label className="model-picker-label">Model</label>
            {models.loading ? (
              <div className="model-loading"><Loader2 size={15} className="spin" /> 正在读取模型配置…</div>
            ) : models.models.length ? (
              <div className="model-list">
                {models.models.map(model => (
                  <button key={model.id} className={`model-option ${selectedModel === model.id ? 'selected' : ''}`} onClick={() => setSelectedModel(model.id)}>
                    <span><strong>{model.name}</strong><small>{model.provider ? `${model.provider} · ` : ''}{model.id}</small></span>
                    <span className="model-check">{selectedModel === model.id ? '✓' : ''}</span>
                  </button>
                ))}
              </div>
            ) : (
              <div className="model-empty">没有读取到该 Agent 的模型配置，将使用 Agent 默认模型。</div>
            )}
            {models.error && <div className="model-error">读取模型失败：{models.error}</div>}
            <button className="primary-button create-chat-button" disabled={models.loading} onClick={() => onSelectAgent(selectedAgent.id, selectedModel)}>
              开始对话
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
