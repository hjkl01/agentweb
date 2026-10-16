import { Bot, ChevronLeft, Folder, Home, Settings, X } from "lucide-react";
import { useEffect, useState } from "react";
import type { Agent, AgentModel } from "../types";
import { api } from "../lib/api";

type Props = {
  open: boolean;
  agents: Agent[];
  onClose: () => void;
  onCreate: (
    agentId: string | undefined,
    model: string | undefined,
    workspace: string,
  ) => void;
  defaultAgentId?: string;
  defaultModel?: string;
  onDefaultChange: (agentId?: string, model?: string) => void;
  onManageAgents: () => void;
};

export function ChatSelectionDialog({
  open,
  agents,
  onClose,
  onCreate,
  defaultAgentId,
  defaultModel,
  onDefaultChange,
  onManageAgents,
}: Props) {
  const installed = agents.filter((a) => a.installed);
  const [agentId, setAgentId] = useState(defaultAgentId || "");
  const [model, setModel] = useState(defaultModel || "");
  const [workspace, setWorkspace] = useState("");
  const [browserPath, setBrowserPath] = useState("");
  const [directories, setDirectories] = useState<string[]>([]);
  const [models, setModels] = useState<AgentModel[]>([]);
  const [loading, setLoading] = useState(false);
  const [browserLoading, setBrowserLoading] = useState(false);
  const [browserError, setBrowserError] = useState("");

  const currentDirectory = browserPath;

  const loadDirectories = async (path?: string) => {
    setBrowserLoading(true);
    setBrowserError("");
    try {
      const query = path ? `?path=${encodeURIComponent(path)}` : "";
      const items = await api<string[]>(`/workspaces${query}`);
      const [current, ...children] = items;
      if (current) {
        setBrowserPath(current);
        setDirectories(children);
        setWorkspace((value) => value || current);
      }
    } catch (error) {
      setBrowserError(error instanceof Error ? error.message : "无法读取目录");
    } finally {
      setBrowserLoading(false);
    }
  };

  useEffect(() => {
    if (open) {
      setAgentId(defaultAgentId || "");
      setModel(defaultModel || "");
      setWorkspace("");
      setBrowserPath("");
      setDirectories([]);
      void loadDirectories();
    }
  }, [open, defaultAgentId, defaultModel]);

  useEffect(() => {
    if (!open || !agentId) {
      setModels([]);
      return;
    }
    let cancelled = false;
    setLoading(true);
    api<AgentModel[]>(`/agents/${agentId}/models`)
      .then((value) => {
        if (!cancelled) setModels(value);
      })
      .catch(() => {
        if (!cancelled) setModels([]);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, agentId]);

  if (!open) return null;

  const chooseAgent = (id: string) => {
    setAgentId(id);
    setModel("");
  };

  const chooseDirectory = (path: string) => {
    setWorkspace(path);
    void loadDirectories(path);
  };

  const selectCurrentDirectory = () => {
    if (currentDirectory) setWorkspace(currentDirectory);
  };

  const goUp = () => {
    if (!currentDirectory) return;
    const trimmed = currentDirectory.replace(/[\\/]+$/, "");
    const slash = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
    if (slash <= 0) {
      void loadDirectories(trimmed);
      return;
    }
    void loadDirectories(trimmed.slice(0, slash));
  };

  const canCreate = !!agentId && !!workspace.trim();

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div
        className="modal new-chat-modal"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-head">
          <div>
            <h3>新建 Chat</h3>
            <p>选择 Agent、模型和工作目录。默认从你的 Home 目录开始。</p>
          </div>
          <button className="icon-button" onClick={onClose} aria-label="关闭">
            <X size={16} />
          </button>
        </div>

        <div className="new-chat-fields">
          <section className="new-chat-field">
            <label className="model-picker-label">工作目录</label>
            <div className="workspace-picker">
              <div className="workspace-picker-toolbar">
                <button
                  type="button"
                  className="workspace-picker-icon"
                  onClick={goUp}
                  disabled={!currentDirectory || browserLoading}
                  title="上一级"
                >
                  <ChevronLeft size={15} />
                </button>
                <button
                  type="button"
                  className="workspace-picker-path"
                  onClick={() => void loadDirectories()}
                  disabled={browserLoading}
                  title="回到 Home"
                >
                  <Home size={14} />
                  <span>{currentDirectory || "Home"}</span>
                </button>
                <button
                  type="button"
                  className="workspace-picker-select"
                  onClick={selectCurrentDirectory}
                  disabled={!currentDirectory || browserLoading}
                >
                  选择此目录
                </button>
              </div>

              <div className="workspace-picker-current">
                <Folder size={14} />
                <span title={currentDirectory}>
                  {currentDirectory || "正在读取…"}
                </span>
              </div>

              <div className="workspace-picker-list">
                {browserLoading && (
                  <div className="workspace-picker-empty">正在读取目录…</div>
                )}
                {!browserLoading && browserError && (
                  <div className="workspace-picker-empty workspace-picker-error">
                    {browserError}
                  </div>
                )}
                {!browserLoading && !browserError && !directories.length && (
                  <div className="workspace-picker-empty">此目录没有子目录</div>
                )}
                {!browserLoading &&
                  !browserError &&
                  directories.map((path) => {
                    const name =
                      path.split(/[\\/]/).filter(Boolean).pop() || path;
                    const selected = workspace === path;
                    return (
                      <button
                        type="button"
                        key={path}
                        className={`workspace-picker-row${selected ? " selected" : ""}`}
                        onClick={() => chooseDirectory(path)}
                      >
                        <Folder size={15} />
                        <span title={path}>{name}</span>
                        {selected && (
                          <span className="workspace-picker-check">✓</span>
                        )}
                      </button>
                    );
                  })}
              </div>

              <div className="workspace-picker-footer">
                <span>当前工作目录</span>
                <strong title={workspace || currentDirectory}>
                  {workspace || currentDirectory || "未选择"}
                </strong>
              </div>
            </div>
          </section>

          <section className="new-chat-field">
            <label className="model-picker-label">Agent</label>
            <select
              value={agentId}
              onChange={(e) => chooseAgent(e.target.value)}
            >
              <option value="">请选择已安装 Agent</option>
              {installed.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name}
                </option>
              ))}
            </select>
          </section>

          <section className="new-chat-field">
            <label className="model-picker-label">Model</label>
            <select
              value={model}
              onChange={(e) => setModel(e.target.value)}
              disabled={!agentId || loading}
            >
              <option value="">
                {loading
                  ? "正在加载模型…"
                  : models.length
                    ? "使用 Agent 默认模型"
                    : "该 Agent 暂无可列出的模型"}
              </option>
              {models.map((item) => (
                <option key={item.id} value={item.id}>
                  {item.name}
                  {item.provider ? ` · ${item.provider}` : ""}
                </option>
              ))}
            </select>
          </section>

          {defaultAgentId === agentId && defaultModel === model && agentId && (
            <div className="modal-empty">
              <p>当前选择为系统默认 Agent + 模型。</p>
            </div>
          )}

          <button
            className="primary-button create-chat-button"
            disabled={!canCreate}
            onClick={() =>
              onCreate(agentId, model || undefined, workspace.trim())
            }
          >
            <Bot size={15} /> 开始对话
          </button>

          {!defaultAgentId && (
            <button
              className="primary-button"
              onClick={() =>
                onDefaultChange(agentId || undefined, model || undefined)
              }
              disabled={!agentId}
            >
              <Settings size={15} /> 设为默认
            </button>
          )}

          {!installed.length && (
            <>
              <div className="modal-empty">
                <p>还没有安装 Agent。</p>
              </div>
              <button className="primary-button" onClick={onManageAgents}>
                <Settings size={15} /> 管理 Agent
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
