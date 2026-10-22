import type { Session } from "../types";

/**
 * 会话家族：fork 出来的会话只记录源会话的原生 id，因此父链在浏览器端就能复原。
 * 返回按“父会话在前、分支紧跟、逐层缩进”排好的行；成环或层级过深也不会丢行。
 */
export type SessionRow = { session: Session; depth: number };

export function familyRows(sessions: Session[]): SessionRow[] {
  const byNative = new Map<string, Session>();
  for (const session of sessions) {
    if (session.native_session_id)
      byNative.set(session.native_session_id, session);
  }
  const hasParent = new Set<string>();
  const childrenOf = new Map<string, Session[]>();
  for (const session of sessions) {
    const parent = session.fork_from_native_session_id
      ? byNative.get(session.fork_from_native_session_id)
      : undefined;
    if (!parent || parent.id === session.id) continue;
    hasParent.add(session.id);
    const list = childrenOf.get(parent.id);
    if (list) list.push(session);
    else childrenOf.set(parent.id, [session]);
  }

  const rows: SessionRow[] = [];
  const seen = new Set<string>();
  const push = (session: Session, depth: number) => {
    if (seen.has(session.id) || depth > 8) return;
    seen.add(session.id);
    rows.push({ session, depth });
    for (const child of childrenOf.get(session.id) || [])
      push(child, depth + 1);
  };
  for (const session of sessions)
    if (!hasParent.has(session.id)) push(session, 0);
  // 父会话不在这批（例如父会话已归档而子会话还在项目组里）时仍然要显示。
  for (const session of sessions) push(session, 0);
  return rows;
}
