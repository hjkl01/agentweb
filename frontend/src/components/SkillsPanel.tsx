import { RefreshCw, Search } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { api } from "../lib/api";
import type { PiCommand } from "../types";

const GROUPS = [
  { source: "skill", label: "技能" },
  { source: "prompt", label: "提示模板" },
  { source: "extension", label: "扩展命令" },
] as const;

const SCOPE_LABELS: Record<string, string> = {
  user: "用户",
  project: "项目",
  temporary: "临时",
};

type Props = { agentId: string; workspace?: string };

/** Read-only view of the resources Pi loads, listed by Pi itself via its RPC mode. */
export function SkillsPanel({ agentId, workspace }: Props) {
  const [commands, setCommands] = useState<PiCommand[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string>();
  const [filter, setFilter] = useState("");

  const load = useCallback(async () => {
    setLoading(true);
    setError(undefined);
    try {
      const query = workspace ? `?cwd=${encodeURIComponent(workspace)}` : "";
      setCommands(
        await api<PiCommand[]>(`/agents/${agentId}/commands${query}`),
      );
    } catch (error) {
      setCommands([]);
      setError(error instanceof Error ? error.message : String(error));
    } finally {
      setLoading(false);
    }
  }, [agentId, workspace]);

  useEffect(() => {
    void load();
  }, [load]);

  const visible = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    if (!needle) return commands;
    return commands.filter((command) =>
      [command.name, command.description, command.path]
        .filter(Boolean)
        .some((value) => value!.toLowerCase().includes(needle)),
    );
  }, [commands, filter]);

  return (
    <div className="skills-panel">
      <div className="mcp-head">
        <span>
          <Search size={14} /> 命令与技能
        </span>
        <button type="button" onClick={() => void load()} disabled={loading}>
          <RefreshCw size={12} className={loading ? "spin" : undefined} />
          {loading ? "加载中" : "刷新"}
        </button>
      </div>
      <input
        className="skills-filter"
        value={filter}
        onChange={(event) => setFilter(event.target.value)}
        placeholder="过滤名称、描述或路径…"
        aria-label="过滤命令"
      />
      {error && <p className="mcp-row-error">{error}</p>}
      {!error && !loading && !commands.length && (
        <p className="mcp-hint">没有发现技能、提示模板或扩展命令。</p>
      )}
      {GROUPS.map(({ source, label }) => {
        const items = visible.filter((command) => command.source === source);
        if (!items.length) return null;
        return (
          <section key={source} className="skills-group">
            <h4>
              {label} <span className="auth-badge">{items.length}</span>
            </h4>
            <div className="skills-list">
              {items.map((command) => (
                <div key={command.name} className="skills-row">
                  <div className="skills-row-head">
                    <code>{command.name}</code>
                    {command.origin === "package" && (
                      <span className="auth-badge">包</span>
                    )}
                    {command.scope && (
                      <span className="auth-badge">
                        {SCOPE_LABELS[command.scope] || command.scope}
                      </span>
                    )}
                  </div>
                  {command.description && (
                    <p className="skills-desc">{command.description}</p>
                  )}
                  {command.path && (
                    <p className="skills-path" title={command.path}>
                      {command.path}
                    </p>
                  )}
                </div>
              ))}
            </div>
          </section>
        );
      })}
      <p className="mcp-hint">
        在会话中用 /名称 触发。这里的列表来自 pi 自身：项目级技能与模板需要先在
        pi 中信任该目录（pi -a）才会出现。
      </p>
    </div>
  );
}
