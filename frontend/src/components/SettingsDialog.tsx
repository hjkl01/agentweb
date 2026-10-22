import {
  BellRing,
  Brain,
  Check,
  FileCog,
  KeyRound,
  Package,
  Plug,
  RefreshCw,
  Save,
  SlidersHorizontal,
  Type,
  Volume2,
  Wand2,
  X,
} from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../types";
import type { Agent, AgentModel } from "../types";
import {
  CHAT_CONTENT_WIDTH,
  CHAT_FONT_SIZE,
  type Preferences,
} from "../lib/preferences";
import { McpPanel } from "./McpPanel";
import { PluginsPanel } from "./PluginsPanel";
import { SkillsPanel } from "./SkillsPanel";

type ProviderAuth = {
  provider: string;
  status: string;
  auth_type?: string;
  reason?: string;
};

type ConfigView = {
  agent_id: string;
  kind: string;
  capabilities: {
    models: boolean;
    native_config: boolean;
    skills: boolean;
    agents_md: boolean;
  };
  known_paths: string[];
  file: { path: string; exists: boolean; content: string; editable: boolean };
};

type Props = {
  open: boolean;
  agents: Agent[];
  agent?: Agent;
  workspace?: string;
  preferences: Preferences;
  onUpdate: (patch: Partial<Preferences>) => void;
  onTestSound: () => void;
  onClose: () => void;
};

const SECTIONS = [
  { id: "general", label: "通用", icon: SlidersHorizontal },
  { id: "skills", label: "技能与模板", icon: Wand2 },
  { id: "plugins", label: "插件", icon: Package },
  { id: "mcp", label: "MCP", icon: Plug },
  { id: "config", label: "原生配置", icon: FileCog },
] as const;

type SectionId = (typeof SECTIONS)[number]["id"];

/** 设置是唯一的入口：通用偏好存在浏览器，其余分区看当前选中的 Agent。 */
export function SettingsDialog({
  open,
  agents,
  agent,
  workspace,
  preferences,
  onUpdate,
  onTestSound,
  onClose,
}: Props) {
  const [activeId, setActiveId] = useState<SectionId>("general");
  // 分区首次打开才挂载；之后保持挂载，切回来不重新拉 Pi 的数据。
  const [visited, setVisited] = useState<SectionId[]>(["general"]);
  const [agentId, setAgentId] = useState<string>();
  useEffect(() => {
    if (open) setAgentId(agent?.id);
  }, [open, agent]);
  if (!open) return null;

  const current =
    agents.find((item) => item.id === agentId) ??
    agents.find((item) => item.kind === "pi") ??
    agents[0];
  const show = (id: SectionId) => {
    setActiveId(id);
    setVisited((list) => (list.includes(id) ? list : [...list, id]));
  };
  const notPi = (
    <div className="settings-empty">
      当前 Agent 不支持这一项：技能、插件、MCP 由 Pi 运行时提供。
    </div>
  );

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div
        className="modal settings-modal"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="modal-head settings-head">
          <div>
            <h3>设置</h3>
            <p>
              通用偏好只保存在当前浏览器；技能、插件与 MCP 由 Pi 运行时提供。
            </p>
          </div>
          {agents.length > 1 && (
            <label className="agent-pick">
              Agent
              <select
                value={current?.id ?? ""}
                onChange={(event) => setAgentId(event.target.value)}
              >
                {agents.map((item) => (
                  <option key={item.id} value={item.id}>
                    {item.name}
                  </option>
                ))}
              </select>
            </label>
          )}
          <button className="icon-button" onClick={onClose}>
            <X size={16} />
          </button>
        </div>
        <div className="settings-body">
          <nav className="settings-nav">
            {SECTIONS.map((section) => (
              <button
                key={section.id}
                className={section.id === activeId ? "active" : undefined}
                onClick={() => show(section.id)}
              >
                <section.icon size={15} /> {section.label}
              </button>
            ))}
          </nav>
          <div className="settings-sections">
            <div className="settings-section" hidden={activeId !== "general"}>
              <span className="pref-row-title">
                <Type size={14} /> 对话外观
              </span>
              <div className="settings-field">
                <div className="settings-field-head">
                  <span>正文字号</span>
                  <span>
                    <output>{preferences.chatFontSize}px</output>
                    <button
                      className="settings-reset"
                      onClick={() =>
                        onUpdate({ chatFontSize: CHAT_FONT_SIZE.default })
                      }
                    >
                      重置
                    </button>
                  </span>
                </div>
                <input
                  type="range"
                  min={CHAT_FONT_SIZE.min}
                  max={CHAT_FONT_SIZE.max}
                  step={1}
                  value={preferences.chatFontSize}
                  onChange={(event) =>
                    onUpdate({ chatFontSize: Number(event.target.value) })
                  }
                />
              </div>
              <div className="settings-field">
                <div className="settings-field-head">
                  <span>内容宽度</span>
                  <span>
                    <output>{preferences.chatContentWidth}px</output>
                    <button
                      className="settings-reset"
                      onClick={() =>
                        onUpdate({
                          chatContentWidth: CHAT_CONTENT_WIDTH.default,
                        })
                      }
                    >
                      重置
                    </button>
                  </span>
                </div>
                <input
                  type="range"
                  min={CHAT_CONTENT_WIDTH.min}
                  max={CHAT_CONTENT_WIDTH.max}
                  step={20}
                  value={preferences.chatContentWidth}
                  onChange={(event) =>
                    onUpdate({ chatContentWidth: Number(event.target.value) })
                  }
                />
              </div>
              <span className="pref-note">
                字号只作用于对话正文与执行过程，输入框和侧栏跟着界面整体缩放走（浏览器
                Ctrl +/-）。
              </span>
              <span className="pref-row-title">
                <Brain size={14} /> 对话
              </span>
              <label className="pref-toggle">
                <input
                  type="checkbox"
                  checked={preferences.thinkingExpanded}
                  onChange={(event) =>
                    onUpdate({ thinkingExpanded: event.target.checked })
                  }
                />
                工具循环里的「思考过程」默认展开
              </label>
              <label className="pref-toggle">
                <input
                  type="checkbox"
                  checked={preferences.quoteSelection}
                  onChange={(event) =>
                    onUpdate({ quoteSelection: event.target.checked })
                  }
                />
                选中对话文字后显示「引用选区」按钮
              </label>
              <div className="settings-choices">
                <span>发送键</span>
                <label>
                  <input
                    type="radio"
                    name="sendMode"
                    checked={preferences.sendMode === "enter"}
                    onChange={() => onUpdate({ sendMode: "enter" })}
                  />
                  Enter 发送，Shift + Enter 换行
                </label>
                <label>
                  <input
                    type="radio"
                    name="sendMode"
                    checked={preferences.sendMode === "ctrlEnter"}
                    onChange={() => onUpdate({ sendMode: "ctrlEnter" })}
                  />
                  Ctrl / Cmd + Enter 发送，Enter 换行
                </label>
              </div>
              <span className="pref-row-title">
                <BellRing size={14} /> 完成提示音
              </span>
              <label className="pref-toggle">
                <input
                  type="checkbox"
                  checked={preferences.completionSound}
                  onChange={(event) =>
                    onUpdate({ completionSound: event.target.checked })
                  }
                />
                会话从运行回到空闲时响一声（在别的标签页也能听见）
              </label>
              <button className="pref-test" onClick={onTestSound}>
                <Volume2 size={13} /> 试听
              </button>
              <span className="pref-note">
                偏好换设备互不影响。深色主题与界面语言暂未提供。
              </span>
            </div>

            <div
              className="settings-section stretch"
              hidden={activeId !== "skills"}
            >
              {visited.includes("skills") &&
                (current?.kind === "pi" ? (
                  <SkillsPanel agentId={current.id} workspace={workspace} />
                ) : (
                  notPi
                ))}
            </div>
            <div
              className="settings-section stretch"
              hidden={activeId !== "plugins"}
            >
              {visited.includes("plugins") &&
                (current?.kind === "pi" ? (
                  <PluginsPanel agentId={current.id} workspace={workspace} />
                ) : (
                  notPi
                ))}
            </div>
            <div
              className="settings-section stretch"
              hidden={activeId !== "mcp"}
            >
              {visited.includes("mcp") &&
                (current?.kind === "pi" ? (
                  <McpPanel agentId={current.id} />
                ) : (
                  notPi
                ))}
            </div>
            <div
              className="settings-section stretch"
              hidden={activeId !== "config"}
            >
              {visited.includes("config") &&
                (current ? (
                  <ConfigSection agent={current} />
                ) : (
                  <div className="settings-empty">还没有安装任何 Agent。</div>
                ))}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

/** Agent 原生配置文件的读写；保存后不需要再做格式转换。 */
function ConfigSection({ agent }: { agent: Agent }) {
  const [path, setPath] = useState("");
  const [content, setContent] = useState("");
  const [knownPaths, setKnownPaths] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState<string>();
  const [error, setError] = useState<string>();
  const [auths, setAuths] = useState<ProviderAuth[]>([]);
  const [authBusy, setAuthBusy] = useState(false);
  const [authError, setAuthError] = useState<string>();

  useEffect(() => {
    setLoading(true);
    setError(undefined);
    setMessage(undefined);
    setAuths([]);
    setAuthError(undefined);
    api<ConfigView>(`/agents/${agent.id}/config`)
      .then((view) => {
        setPath(view.file.path);
        setContent(view.file.content);
        setKnownPaths(view.known_paths);
      })
      .catch((error) =>
        setError(error instanceof Error ? error.message : String(error)),
      )
      .finally(() => setLoading(false));
  }, [agent]);

  const loadPath = async (nextPath: string) => {
    setPath(nextPath);
    setLoading(true);
    setError(undefined);
    setMessage(undefined);
    try {
      const view = await api<ConfigView>(
        `/agents/${agent.id}/config?path=${encodeURIComponent(nextPath)}`,
      );
      setContent(view.file.content);
      setKnownPaths(view.known_paths);
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error));
    } finally {
      setLoading(false);
    }
  };

  const save = async () => {
    if (!path.trim()) return;
    setSaving(true);
    setError(undefined);
    setMessage(undefined);
    try {
      const view = await api<ConfigView>(`/agents/${agent.id}/config`, {
        method: "PUT",
        body: JSON.stringify({ path, content }),
      });
      setPath(view.file.path);
      setContent(view.file.content);
      setMessage("配置已保存");
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error));
    } finally {
      setSaving(false);
    }
  };

  const checkAuth = async () => {
    setAuthBusy(true);
    setAuthError(undefined);
    try {
      const models = await api<AgentModel[]>(`/agents/${agent.id}/models`);
      const providers = [
        ...new Set(
          models
            .map((model) => model.provider)
            .filter((provider): provider is string => Boolean(provider)),
        ),
      ].sort();
      const results = await Promise.all(
        providers.map((provider) =>
          api<ProviderAuth>(
            `/agents/${agent.id}/auth?provider=${encodeURIComponent(provider)}`,
          ).catch((error): ProviderAuth => ({
            provider,
            status: "unknown",
            reason: error instanceof Error ? error.message : String(error),
          })),
        ),
      );
      setAuths(results);
    } catch (error) {
      setAuthError(error instanceof Error ? error.message : String(error));
    } finally {
      setAuthBusy(false);
    }
  };

  return (
    <>
      <div className="config-hint">
        Codex 使用 <code>CODEX_HOME/config.toml</code>；Pi 使用{" "}
        <code>~/.pi/agent/models.json</code>。其他 Agent
        如果无法自动识别，可直接填写配置文件路径。
      </div>
      <div className="config-path-row">
        <input
          value={path}
          onChange={(event) => setPath(event.target.value)}
          placeholder="配置文件路径，例如 ~/.config/opencode/opencode.json"
        />
        <button onClick={save} disabled={loading || saving || !path.trim()}>
          <Save size={14} />
          {saving ? "保存中…" : "保存"}
        </button>
      </div>
      {agent.kind === "pi" && (
        <div className="provider-auth">
          <div className="provider-auth-head">
            <span>
              <KeyRound size={14} /> Provider 凭据
            </span>
            <button onClick={checkAuth} disabled={authBusy}>
              <RefreshCw size={13} className={authBusy ? "spin" : undefined} />
              {authBusy ? "检查中…" : "检查"}
            </button>
          </div>
          {authError && <span className="error-text">{authError}</span>}
          {auths.length > 0 && (
            <div className="provider-auth-list">
              {auths.map((auth) => (
                <div key={auth.provider} className="provider-auth-row">
                  <code>{auth.provider}</code>
                  <span className={`auth-badge auth-${auth.status}`}>
                    {auth.status}
                  </span>
                  <small>{auth.auth_type || auth.reason || ""}</small>
                </div>
              ))}
            </div>
          )}
          {!authBusy && !authError && auths.length === 0 && (
            <span className="provider-auth-hint">
              在 Pi 中执行 <code>/login</code>，或在 models.json /
              环境变量里配置 API Key。
            </span>
          )}
        </div>
      )}
      {knownPaths.length > 0 && (
        <div className="config-known-paths">
          <span>检测/建议路径：</span>
          {knownPaths.map((item) => (
            <button key={item} onClick={() => loadPath(item)}>
              {item}
            </button>
          ))}
        </div>
      )}
      <textarea
        className="config-editor"
        value={content}
        onChange={(event) => setContent(event.target.value)}
        spellCheck={false}
        placeholder="配置文件内容…"
        disabled={loading}
      />
      <div className="config-footer">
        {loading && <span>读取中…</span>}
        {message && (
          <span className="success-text">
            <Check size={14} />
            {message}
          </span>
        )}
        {error && <span className="error-text">{error}</span>}
        <span>仅允许保存到 HOME 目录内，单文件最大 1 MiB。</span>
      </div>
    </>
  );
}
