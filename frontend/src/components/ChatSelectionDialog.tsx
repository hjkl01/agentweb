import {
  Bot,
  ChevronLeft,
  Folder,
  GitBranch,
  Home,
  Settings,
  Trash2,
  X,
} from "lucide-react";
import { useEffect, useState } from "react";
import type { Agent, AgentModel, WorktreeInfo, WorktreesView } from "../types";
import { ApiError, api } from "../lib/api";

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
  defaultWorkspace?: string;
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
  defaultWorkspace,
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
  const [worktrees, setWorktrees] = useState<WorktreeInfo[]>([]);
  const [isGit, setIsGit] = useState(false);
  const [projectRoot, setProjectRoot] = useState("");
  const [newBranch, setNewBranch] = useState("");
  const [worktreeBusy, setWorktreeBusy] = useState(false);
  const [worktreeError, setWorktreeError] = useState("");
  const [loading, setLoading] = useState(false);
  const [browserLoading, setBrowserLoading] = useState(false);
  const [browserError, setBrowserError] = useState("");
  const [newFolder, setNewFolder] = useState("");
  const [folderBusy, setFolderBusy] = useState(false);
  const [folderError, setFolderError] = useState("");

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
      // loadDirectories below writes the canonical path, so a default value
      // carrying a literal `~` must not win over it.
      setWorkspace("");
      setBrowserPath("");
      setDirectories([]);
      setWorktrees([]);
      setIsGit(false);
      setProjectRoot("");
      setNewBranch("");
      setWorktreeError("");
      setNewFolder("");
      setFolderError("");
      void loadDirectories(defaultWorkspace || undefined);
    }
  }, [open, defaultAgentId, defaultModel, defaultWorkspace]);

  // Worktrees are resolved from whatever directory is on screen, so the
  // picker reflects the repo even before a workspace is selected.
  useEffect(() => {
    if (!open || !currentDirectory) {
      setWorktrees([]);
      setIsGit(false);
      setProjectRoot("");
      return;
    }
    let cancelled = false;
    api<WorktreesView>(`/worktrees?cwd=${encodeURIComponent(currentDirectory)}`)
      .then((view) => {
        if (cancelled) return;
        setWorktrees(view.worktrees);
        setIsGit(view.is_git);
        setProjectRoot(view.project_root || "");
      })
      .catch(() => {
        if (cancelled) return;
        setWorktrees([]);
        setIsGit(false);
        setProjectRoot("");
      });
    return () => {
      cancelled = true;
    };
  }, [open, currentDirectory]);

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

  const createFolder = async () => {
    const name = newFolder.trim();
    if (!name || !currentDirectory) return;
    setFolderBusy(true);
    setFolderError("");
    try {
      const created = await api<string>("/workspaces", {
        method: "POST",
        body: JSON.stringify({ parent: currentDirectory, name }),
      });
      setNewFolder("");
      chooseDirectory(created);
    } catch (error) {
      setFolderError(error instanceof Error ? error.message : "创建失败");
    } finally {
      setFolderBusy(false);
    }
  };

  const selectCurrentDirectory = () => {
    if (currentDirectory) setWorkspace(currentDirectory);
  };

  // 删除当前浏览的目录（递归）。后端会拒绝仍在被会话使用的目录。
  const removeDirectory = async () => {
    if (!currentDirectory) return;
    if (
      !window.confirm(
        `删除目录 ${currentDirectory}？其中的文件会被一并永久删除，且无法恢复。`,
      )
    )
      return;
    setFolderBusy(true);
    setFolderError("");
    try {
      await api("/workspaces", {
        method: "DELETE",
        body: JSON.stringify({ path: currentDirectory }),
      });
      setWorkspace((value) => (value === currentDirectory ? "" : value));
      goUp();
    } catch (error) {
      setFolderError(error instanceof Error ? error.message : "删除失败");
    } finally {
      setFolderBusy(false);
    }
  };

  const createWorktree = async () => {
    const branch = newBranch.trim();
    if (!branch || !currentDirectory) return;
    setWorktreeBusy(true);
    setWorktreeError("");
    try {
      const created = await api<WorktreeInfo>("/worktrees", {
        method: "POST",
        body: JSON.stringify({ cwd: currentDirectory, branch }),
      });
      setNewBranch("");
      setWorktrees((list) => [
        ...list.filter((item) => item.path !== created.path),
        created,
      ]);
      chooseDirectory(created.path);
    } catch (error) {
      setWorktreeError(error instanceof Error ? error.message : "创建失败");
    } finally {
      setWorktreeBusy(false);
    }
  };

  const selectedWorktree = worktrees.find(
    (item) => workspace === item.path || currentDirectory === item.path,
  );

  const removeWorktree = async () => {
    const target = selectedWorktree;
    if (!target) return;
    const label = target.branch || target.path;
    if (!window.confirm(`删除 worktree「${label}」？`)) return;
    setWorktreeBusy(true);
    setWorktreeError("");
    const drop = async (force: boolean) => {
      await api("/worktrees", {
        method: "DELETE",
        body: JSON.stringify({
          cwd: currentDirectory,
          path: target.path,
          force,
        }),
      });
      setWorktrees((list) => list.filter((item) => item.path !== target.path));
      if (projectRoot) chooseDirectory(projectRoot);
      else void loadDirectories();
    };
    try {
      await drop(false);
    } catch (error) {
      if (error instanceof ApiError && error.status === 409) {
        if (
          window.confirm(`${error.message}\n\n强制删除？未提交的改动将丢失。`)
        ) {
          try {
            await drop(true);
          } catch (retry) {
            setWorktreeError(
              retry instanceof Error ? retry.message : "删除失败",
            );
          }
        }
      } else {
        setWorktreeError(error instanceof Error ? error.message : "删除失败");
      }
    } finally {
      setWorktreeBusy(false);
    }
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

              <div className="workspace-picker-toolbar workspace-folder-create">
                <input
                  value={newFolder}
                  placeholder="新建文件夹名"
                  onChange={(e) => setNewFolder(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") void createFolder();
                  }}
                  disabled={folderBusy}
                />
                <button
                  type="button"
                  className="workspace-picker-select"
                  onClick={() => void createFolder()}
                  disabled={folderBusy || !newFolder.trim()}
                >
                  {folderBusy ? "创建中…" : "新建文件夹"}
                </button>
                <button
                  type="button"
                  className="workspace-picker-icon workspace-picker-danger"
                  onClick={() => void removeDirectory()}
                  disabled={folderBusy || !currentDirectory}
                  title={`删除目录 ${currentDirectory || ""}`}
                  aria-label="删除当前目录"
                >
                  <Trash2 size={15} />
                </button>
              </div>
              {folderError && (
                <div className="workspace-picker-empty workspace-picker-error">
                  {folderError}
                </div>
              )}
              <div className="workspace-picker-footer">
                <span>当前工作目录</span>
                <strong title={workspace || currentDirectory}>
                  {workspace || currentDirectory || "未选择"}
                </strong>
              </div>
            </div>
          </section>

          {isGit && (
            <section className="new-chat-field">
              <label className="model-picker-label">Git Worktree</label>
              {projectRoot && (
                <div className="worktree-project" title={projectRoot}>
                  项目：{projectRoot}
                </div>
              )}
              <div className="workspace-picker-list worktree-list">
                {worktrees.map((item) => {
                  const name =
                    item.branch ||
                    item.path.split(/[\\/]/).filter(Boolean).pop() ||
                    item.path;
                  const selected =
                    workspace === item.path || currentDirectory === item.path;
                  return (
                    <button
                      type="button"
                      key={item.path}
                      className={`workspace-picker-row${selected ? " selected" : ""}`}
                      onClick={() => chooseDirectory(item.path)}
                      title={item.path}
                    >
                      <GitBranch size={15} />
                      <span>{name}</span>
                      {item.is_main && <span className="worktree-tag">主</span>}
                      {!item.available && (
                        <span className="worktree-tag muted">缺失</span>
                      )}
                      {selected && (
                        <span className="workspace-picker-check">✓</span>
                      )}
                    </button>
                  );
                })}
                {!worktrees.length && (
                  <div className="workspace-picker-empty">暂无 worktree</div>
                )}
              </div>
              <div className="workspace-picker-toolbar worktree-create">
                <input
                  value={newBranch}
                  placeholder="新分支名，如 feature/x"
                  onChange={(e) => setNewBranch(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") void createWorktree();
                  }}
                  disabled={worktreeBusy}
                />
                <button
                  type="button"
                  className="workspace-picker-select"
                  onClick={() => void createWorktree()}
                  disabled={worktreeBusy || !newBranch.trim()}
                >
                  {worktreeBusy ? "创建中…" : "新建 worktree"}
                </button>
              </div>
              {worktreeError && (
                <div className="workspace-picker-empty workspace-picker-error">
                  {worktreeError}
                </div>
              )}
              {selectedWorktree &&
                !selectedWorktree.is_main &&
                selectedWorktree.available && (
                  <button
                    type="button"
                    className="worktree-remove"
                    onClick={() => void removeWorktree()}
                    disabled={worktreeBusy}
                  >
                    <Trash2 size={13} /> 删除 worktree「
                    {selectedWorktree.branch || selectedWorktree.path}」
                  </button>
                )}
            </section>
          )}

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
