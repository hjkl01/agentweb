import { Download, Package, Plus, RefreshCw, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { api } from "../types";
import type { PluginPackage } from "../types";

type Props = { agentId: string; workspace?: string };

/** Pi 包列表：与 MCP 面板共用同一套列表样式（mcp-* 是通用行样式）。 */
export function PluginsPanel({ agentId, workspace }: Props) {
  const [packages, setPackages] = useState<PluginPackage[]>();
  const [busy, setBusy] = useState(false);
  const [pending, setPending] = useState<string>();
  const [error, setError] = useState<string>();
  const [source, setSource] = useState("");
  const [local, setLocal] = useState(false);

  const query = workspace ? `?cwd=${encodeURIComponent(workspace)}` : "";
  const path = `/agents/${agentId}/plugins${query}`;

  const load = useCallback(async () => {
    setBusy(true);
    setError(undefined);
    try {
      setPackages(await api<PluginPackage[]>(path));
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  }, [path]);

  useEffect(() => {
    setPackages(undefined);
    setError(undefined);
    setPending(undefined);
    setSource("");
    void load();
  }, [load]);

  const mutate = async (key: string, body: unknown) => {
    setPending(key);
    setError(undefined);
    try {
      setPackages(
        await api<PluginPackage[]>(path, {
          method: "POST",
          body: JSON.stringify(body),
        }),
      );
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error));
    } finally {
      setPending(undefined);
    }
  };

  const install = async () => {
    const trimmed = source.trim();
    if (!trimmed) return;
    await mutate("__install", { action: "install", source: trimmed, local });
    setSource("");
  };

  const remove = async (target: string, scope: string) => {
    if (!window.confirm(`移除 ${target}？`)) return;
    await mutate(target, {
      action: "remove",
      source: target,
      local: scope === "project",
    });
  };

  return (
    <div className="mcp-panel">
      <div className="mcp-head">
        <span>
          <Package size={14} /> 插件（Pi 包）
        </span>
        <span className="plugins-head-actions">
          <button
            onClick={() => mutate("__update", { action: "update" })}
            disabled={!packages || pending === "__update"}
          >
            <Download size={13} /> 更新全部
          </button>
          <button onClick={load} disabled={busy}>
            <RefreshCw size={13} className={busy ? "spin" : undefined} />
            {busy ? "读取中…" : packages ? "刷新" : "加载"}
          </button>
        </span>
      </div>
      {error && <span className="error-text">{error}</span>}
      {packages && (
        <>
          {packages.length === 0 && (
            <span className="mcp-hint">
              还没有安装插件包。可在下方按来源安装，或直接使用 pi install。
            </span>
          )}
          {packages.length > 0 && (
            <div className="mcp-list">
              {packages.map((entry) => (
                <div key={`${entry.scope}:${entry.source}`} className="mcp-row">
                  <div className="mcp-row-main">
                    <span>{entry.source}</span>
                    <span
                      className={`auth-badge ${entry.scope === "project" ? "auth-not_ready" : "auth-ready"}`}
                    >
                      {entry.scope === "project" ? "项目" : "用户"}
                    </span>
                    <small>{entry.path || "未安装"}</small>
                  </div>
                  <div className="mcp-row-actions">
                    <button
                      onClick={() =>
                        mutate(entry.source, {
                          action: "update",
                          source: entry.source,
                        })
                      }
                      disabled={pending === entry.source}
                    >
                      <Download size={13} /> 更新
                    </button>
                    <button
                      onClick={() => remove(entry.source, entry.scope)}
                      disabled={pending === entry.source}
                    >
                      <Trash2 size={13} /> 移除
                    </button>
                  </div>
                </div>
              ))}
            </div>
          )}
          <div className="mcp-add">
            <input
              value={source}
              onChange={(event) => setSource(event.target.value)}
              placeholder="npm:@scope/pkg、git:github.com/user/repo 或 ./local/path"
              aria-label="包来源"
            />
            <button
              onClick={install}
              disabled={pending === "__install" || !source.trim()}
            >
              <Plus size={13} /> 安装
            </button>
          </div>
          <label className="plugins-local">
            <input
              type="checkbox"
              checked={local}
              onChange={(event) => setLocal(event.target.checked)}
            />
            安装到项目（.pi/settings.json）
          </label>
          <span className="mcp-hint">
            列表与安装都交给 pi 自己（settings.json）。项目级包需要先在 pi
            中信任该目录（pi -a）才能读写。
          </span>
        </>
      )}
    </div>
  );
}
