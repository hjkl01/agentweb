import { Bot, CheckCircle2, Download, LoaderCircle, Terminal, X } from 'lucide-react';
import { useState } from 'react';
import type { Agent, NodeInfo } from '../types';

type Feedback = { version: string; state: 'success' | 'error'; message: string };
type Props = {
  open: boolean; agents: Agent[]; node?: NodeInfo; nodeVersion: string; installingNode: boolean; installingAgent?: string; nodeInstallFeedback?: Feedback;
  onClose: () => void; onNodeVersionChange: (version: string) => void; onInstallNode: (version?: string) => void; onInstallAgent: (id: string) => void; onInstallCustomAgent: (command: string) => void;
};

export function AgentCatalog({ open, agents, node, nodeVersion, installingNode, installingAgent, nodeInstallFeedback, onClose, onNodeVersionChange, onInstallNode, onInstallAgent, onInstallCustomAgent }: Props) {
  const [customCommand, setCustomCommand] = useState('');
  if (!open) return null;
  const installed = new Set(node?.installed || []);

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="agent-panel" onClick={event => event.stopPropagation()}>
        <div className="modal-head"><div><h3>Agents & runtimes</h3><p>Node.js 和 Agent 都按需安装，安装结果保存在 /data。</p></div><button className="icon-button" onClick={onClose}><X size={16} /></button></div>
        <div className="runtime-card">
          <div className="card-title"><div><strong>Node.js</strong><span>可用版本 / 已安装版本</span></div><span>{node?.active || 'not installed'}</span></div>
          <div className="runtime-row"><input value={nodeVersion} onChange={event => onNodeVersionChange(event.target.value)} placeholder="输入版本，例如 22.16.0 或 22" /><button onClick={() => onInstallNode()} disabled={!nodeVersion.trim() || installingNode}>{installingNode ? 'Installing…' : '安装输入版本'}</button></div>
          <div className="node-version-list">
            {!node?.available?.length && <span className="installed-note">正在获取可用 Node.js 版本…</span>}
            {node?.available?.map(version => {
              const isInstalled = installed.has(version);
              const isInstalling = installingNode && nodeVersion === version;
              return <div className={`node-version-row ${isInstalled ? 'installed' : ''}`} key={version}>
                <span className="node-version-name">Node.js {version}</span>
                {isInstalled ? <span className="node-status"><CheckCircle2 size={14} />已安装</span> : <button className="node-install-button" disabled={installingNode} onClick={() => { onNodeVersionChange(version); onInstallNode(version); }}>{isInstalling ? <><LoaderCircle size={13} className="spin" />安装中…</> : <><Download size={13} />安装</>}</button>}
              </div>;
            })}
          </div>
          {nodeInstallFeedback && <div className={`node-install-feedback ${nodeInstallFeedback.state}`} role="status"><CheckCircle2 size={15} /><span>{nodeInstallFeedback.message}</span></div>}
        </div>
        <div className="modal-head"><div><h3>热门 Agent</h3><p>常用 CLI 可以直接一键安装；其他 Agent 可以使用下面的自定义安装命令。</p></div></div>
        <div className="agent-list">
          {agents.map(agent => <div className="agent-card" key={agent.id}>
            <div className="agent-icon"><Bot size={17} /></div>
            <div className="agent-info"><div className="agent-name">{agent.name} {agent.installed && <CheckCircle2 size={14} />}</div><p>{agent.description || 'No description'}</p>{agent.install_command && <code>{agent.install_command}</code>}</div>
            {agent.installed ? <span className="badge installed">Installed</span> : <button onClick={() => onInstallAgent(agent.id)} disabled={installingAgent === agent.id}><Download size={14} />{installingAgent === agent.id ? 'Installing…' : '一键安装'}</button>}
          </div>)}
        </div>
        <div className="custom-agent-card">
          <div className="card-title"><div><strong>自定义 Agent</strong><span>输入任意安装命令</span></div><Terminal size={17} /></div>
          <p>例如 npm、pnpm、bun、pip、curl 等命令都可以直接执行。</p>
          <div className="runtime-row"><input value={customCommand} onChange={event => setCustomCommand(event.target.value)} onKeyDown={event => { if (event.key === 'Enter') { event.preventDefault(); onInstallCustomAgent(customCommand); } }} placeholder="例如 npm install -g @qwen-code/qwen-code" /><button onClick={() => onInstallCustomAgent(customCommand)} disabled={!customCommand.trim() || installingAgent === 'custom'}>{installingAgent === 'custom' ? 'Installing…' : '安装'}</button></div>
        </div>
      </div>
    </div>
  );
}
