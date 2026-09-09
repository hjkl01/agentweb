import { Check, FileCog, Save, X } from 'lucide-react';
import { useEffect, useState } from 'react';
import { api } from '../types';
import type { Agent } from '../types';

type ConfigView = {
  agent_id: string;
  kind: string;
  capabilities: { models: boolean; native_config: boolean; skills: boolean; agents_md: boolean };
  known_paths: string[];
  file: { path: string; exists: boolean; content: string; editable: boolean };
};

type Props = { open: boolean; agent?: Agent; onClose: () => void };

export function AgentConfigDialog({ open, agent, onClose }: Props) {
  const [path, setPath] = useState('');
  const [content, setContent] = useState('');
  const [knownPaths, setKnownPaths] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState<string>();
  const [error, setError] = useState<string>();

  useEffect(() => {
    if (!open || !agent) return;
    setLoading(true); setError(undefined); setMessage(undefined);
    api<ConfigView>(`/agents/${agent.id}/config`)
      .then(view => { setPath(view.file.path); setContent(view.file.content); setKnownPaths(view.known_paths); })
      .catch(error => setError(error instanceof Error ? error.message : String(error)))
      .finally(() => setLoading(false));
  }, [open, agent]);

  if (!open || !agent) return null;

  const loadPath = async (nextPath: string) => {
    setPath(nextPath); setLoading(true); setError(undefined); setMessage(undefined);
    try {
      const view = await api<ConfigView>(`/agents/${agent.id}/config?path=${encodeURIComponent(nextPath)}`);
      setContent(view.file.content); setKnownPaths(view.known_paths);
    } catch (error) { setError(error instanceof Error ? error.message : String(error)); }
    finally { setLoading(false); }
  };

  const save = async () => {
    if (!path.trim()) return;
    setSaving(true); setError(undefined); setMessage(undefined);
    try {
      const view = await api<ConfigView>(`/agents/${agent.id}/config`, { method: 'PUT', body: JSON.stringify({ path, content }) });
      setPath(view.file.path); setContent(view.file.content); setMessage('配置已保存');
    } catch (error) { setError(error instanceof Error ? error.message : String(error)); }
    finally { setSaving(false); }
  };

  return <div className="modal-backdrop" onClick={onClose}>
    <div className="agent-config-panel" onClick={event => event.stopPropagation()}>
      <div className="modal-head">
        <div><h3><FileCog size={17} /> 配置 {agent.name}</h3><p>优先使用 Agent 原生配置；保存后无需额外转换。</p></div>
        <button className="icon-button" onClick={onClose}><X size={16} /></button>
      </div>
      <div className="config-hint">Codex 使用 <code>CODEX_HOME/config.toml</code>；Pi 使用 <code>~/.pi/agent/models.json</code>。其他 Agent 如果无法自动识别，可直接填写配置文件路径。</div>
      <div className="config-path-row">
        <input value={path} onChange={event => setPath(event.target.value)} placeholder="配置文件路径，例如 ~/.config/opencode/opencode.json" />
        <button onClick={save} disabled={loading || saving || !path.trim()}><Save size={14} />{saving ? '保存中…' : '保存'}</button>
      </div>
      {knownPaths.length > 0 && <div className="config-known-paths"><span>检测/建议路径：</span>{knownPaths.map(item => <button key={item} onClick={() => loadPath(item)}>{item}</button>)}</div>}
      <textarea className="config-editor" value={content} onChange={event => setContent(event.target.value)} spellCheck={false} placeholder="配置文件内容…" disabled={loading} />
      <div className="config-footer">
        {loading && <span>读取中…</span>}
        {message && <span className="success-text"><Check size={14} />{message}</span>}
        {error && <span className="error-text">{error}</span>}
        <span>仅允许保存到 HOME 目录内，单文件最大 1 MiB。</span>
      </div>
    </div>
  </div>;
}
