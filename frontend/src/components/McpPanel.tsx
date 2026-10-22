import { Plug, Plus, RefreshCw, Trash2, Unplug } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { api } from "../types";
import type { McpView } from "../types";

type Props = { agentId: string };

const stateClass = (state?: string) =>
  state === "connected"
    ? "auth-ready"
    : state === "failed"
      ? "auth-invalid"
      : "auth-not_ready";

export function McpPanel({ agentId }: Props) {
  const [view, setView] = useState<McpView>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const [pending, setPending] = useState<string>();
  const [name, setName] = useState("");
  const [mode, setMode] = useState<"stdio" | "http">("stdio");
  const [target, setTarget] = useState("");
  const [args, setArgs] = useState("");

  const load = useCallback(async () => {
    setBusy(true);
    setError(undefined);
    try {
      setView(await api<McpView>(`/agents/${agentId}/mcp`));
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  }, [agentId]);

  useEffect(() => {
    setView(undefined);
    setError(undefined);
    setPending(undefined);
    setName("");
    setTarget("");
    setArgs("");
    void load();
  }, [load]);

  const mutate = async (
    key: string,
    path: string,
    options: RequestInit,
  ): Promise<void> => {
    setPending(key);
    setError(undefined);
    try {
      setView(await api<McpView>(path, options));
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error));
      throw error;
    } finally {
      setPending(undefined);
    }
  };

  const add = async () => {
    const trimmed = target.trim();
    if (!name.trim() || !trimmed) return;
    try {
      await mutate("__add", `/agents/${agentId}/mcp`, {
        method: "POST",
        body: JSON.stringify(
          mode === "http"
            ? { name: name.trim(), url: trimmed }
            : {
                name: name.trim(),
                command: trimmed,
                args: args.trim() ? args.trim().split(/\s+/) : [],
              },
        ),
      });
      setName("");
      setTarget("");
      setArgs("");
    } catch {
      // The message is already rendered above the list.
    }
  };

  return (
    <div className="mcp-panel">
      <div className="mcp-head">
        <span>
          <Plug size={14} /> MCP 服务器
        </span>
        <button onClick={load} disabled={busy}>
          <RefreshCw size={13} className={busy ? "spin" : undefined} />
          {busy ? "连接中…" : view ? "刷新" : "加载"}
        </button>
      </div>
      {error && <span className="error-text">{error}</span>}
      {view && (
        <>
          {view.servers.length === 0 && (
            <span className="mcp-hint">
              还没有 MCP 服务器。可在下方添加，或直接编辑 mcp.json。
            </span>
          )}
          <div className="mcp-list">
            {view.servers.map((server) => (
              <div key={server.name} className="mcp-row">
                <div className="mcp-row-main">
                  <code>{server.name}</code>
                  <span className={`auth-badge ${stateClass(server.state)}`}>
                    {server.state || "unknown"}
                  </span>
                  <small>{server.transport}</small>
                </div>
                <div className="mcp-row-meta">
                  {server.exposure && <span>exposure: {server.exposure}</span>}
                  {server.tools.length > 0 && (
                    <span title={server.tools.join(", ")}>
                      {server.tools.length} 个工具
                    </span>
                  )}
                  {server.scope && <span>{server.scope}</span>}
                </div>
                {server.error && (
                  <span className="mcp-row-error">{server.error}</span>
                )}
                <div className="mcp-row-actions">
                  <button
                    disabled={pending === server.name}
                    onClick={() =>
                      mutate(
                        server.name,
                        `/agents/${agentId}/mcp/logout?name=${encodeURIComponent(server.name)}`,
                        { method: "POST" },
                      ).catch(() => {})
                    }
                  >
                    <Unplug size={13} /> 登出
                  </button>
                  <button
                    disabled={pending === server.name}
                    onClick={() => {
                      if (
                        !window.confirm(`移除 MCP 服务器「${server.name}」？`)
                      )
                        return;
                      mutate(
                        server.name,
                        `/agents/${agentId}/mcp?name=${encodeURIComponent(server.name)}`,
                        { method: "DELETE" },
                      ).catch(() => {});
                    }}
                  >
                    <Trash2 size={13} /> 移除
                  </button>
                </div>
              </div>
            ))}
          </div>
          <div className="mcp-add">
            <input
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder="名称，例如 filesystem"
              aria-label="MCP 服务器名称"
            />
            <select
              value={mode}
              onChange={(event) =>
                setMode(event.target.value as "stdio" | "http")
              }
              aria-label="传输方式"
            >
              <option value="stdio">命令</option>
              <option value="http">URL</option>
            </select>
            <input
              value={target}
              onChange={(event) => setTarget(event.target.value)}
              placeholder={
                mode === "http" ? "https://example.com/mcp" : "命令，例如 npx"
              }
              aria-label={mode === "http" ? "URL" : "命令"}
            />
            {mode === "stdio" && (
              <input
                value={args}
                onChange={(event) => setArgs(event.target.value)}
                placeholder="参数（空格分隔）"
                aria-label="参数"
              />
            )}
            <button
              onClick={add}
              disabled={pending === "__add" || !name.trim() || !target.trim()}
            >
              <Plus size={13} /> 添加
            </button>
          </div>
          <span className="mcp-hint">
            添加即写入 Pi 的 mcp.json，不写入密钥；需要请求头、OAuth
            或环境变量时请编辑 mcp.json 或使用 <code>pi mcp</code>。
          </span>
        </>
      )}
    </div>
  );
}
