import type { Session } from "../types";

// Sessions are grouped by project. Worktrees created by Agent Web live at
// `<repo>-worktrees/<branch>`, so they collapse back under their main repo.
export const PINNED_KEY = "__pinned__";
// Archived sessions leave their project group for this one, collapsed by default.
export const ARCHIVED_KEY = "__archived__";
const WORKTREE_SUFFIX = "-worktrees";

function projectKey(workspace: string): string {
  const parts = workspace.replace(/[\\/]+$/, "").split(/[\\/]/);
  const parent = parts[parts.length - 2];
  if (parent && parent.endsWith(WORKTREE_SUFFIX)) {
    return [
      ...parts.slice(0, -2),
      parent.slice(0, -WORKTREE_SUFFIX.length),
    ].join("/");
  }
  return workspace;
}

function projectLabel(key: string): string {
  if (key === PINNED_KEY) return "置顶";
  if (key === ARCHIVED_KEY) return "归档";
  const parts = key.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] || key;
}

/** 置顶在最前、归档在最后，其余按项目首次出现的顺序。Map 的插入顺序就是展示顺序。 */
export function groupSessions(
  sessions: Session[],
): { key: string; label: string; sessions: Session[] }[] {
  const byKey = new Map<string, Session[]>();
  const pinned = sessions.filter(
    (session) => session.is_pinned && !session.archived_at,
  );
  if (pinned.length) byKey.set(PINNED_KEY, pinned);
  for (const session of sessions) {
    if (session.is_pinned || session.archived_at) continue;
    const key = projectKey(session.workspace || "");
    const list = byKey.get(key);
    if (list) list.push(session);
    else byKey.set(key, [session]);
  }
  const archived = sessions.filter((session) => session.archived_at);
  if (archived.length) byKey.set(ARCHIVED_KEY, archived);
  return [...byKey].map(([key, group]) => ({
    key,
    label: projectLabel(key),
    sessions: group,
  }));
}
