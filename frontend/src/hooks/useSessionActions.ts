import { useCallback } from "react";
import type { Dispatch, SetStateAction } from "react";
import { api } from "../lib/api";
import type { Session } from "../types";

type Options = {
  setSessions: Dispatch<SetStateAction<Session[]>>;
  markStatusChanged?: (sessionId: string) => void;
};

export function useSessionActions({ setSessions, markStatusChanged }: Options) {
  const createSession = useCallback(
    async (
      agentId: string | undefined,
      model: string | undefined,
      workspace: string,
    ) => {
      const session = await api<Session>("/sessions", {
        method: "POST",
        body: JSON.stringify({
          agent_id: agentId,
          title: "New Chat",
          model,
          workspace,
        }),
      });
      setSessions((items) => [session, ...items]);
      return session;
    },
    [setSessions],
  );
  const createPiFork = useCallback(
    async (sessionId: string) => {
      const session = await api<Session>(`/sessions/${sessionId}/fork`, {
        method: "POST",
      });
      setSessions((items) => [session, ...items]);
      return session;
    },
    [setSessions],
  );
  /** Branches Pi's session at one user message, returning the message text for the editor. */
  const forkFromEntry = useCallback(
    async (sessionId: string, entryId: string) => {
      const result = await api<{ session: Session; text?: string }>(
        `/sessions/${sessionId}/fork-from`,
        { method: "POST", body: JSON.stringify({ entry_id: entryId }) },
      );
      setSessions((items) => [result.session, ...items]);
      return result;
    },
    [setSessions],
  );
  const restartMissingSession = useCallback(
    async (sessionId: string) => {
      const session = await api<Session>(`/sessions/${sessionId}/restart`, {
        method: "POST",
      });
      setSessions((items) =>
        items.map((item) => (item.id === sessionId ? session : item)),
      );
      return session;
    },
    [setSessions],
  );
  const setModel = useCallback(
    async (sessionId: string, model?: string) => {
      const session = await api<Session>(`/sessions/${sessionId}/model`, {
        method: "PUT",
        body: JSON.stringify({ model: model || null }),
      });
      setSessions((items) =>
        items.map((item) => (item.id === sessionId ? session : item)),
      );
      return session;
    },
    [setSessions],
  );
  const setThinking = useCallback(
    async (sessionId: string, thinkingLevel?: string) => {
      const session = await api<Session>(`/sessions/${sessionId}/thinking`, {
        method: "PUT",
        body: JSON.stringify({ thinking_level: thinkingLevel || null }),
      });
      setSessions((items) =>
        items.map((item) => (item.id === sessionId ? session : item)),
      );
      return session;
    },
    [setSessions],
  );
  const setTools = useCallback(
    async (sessionId: string, toolPreset?: string) => {
      const session = await api<Session>(`/sessions/${sessionId}/tools`, {
        method: "PUT",
        body: JSON.stringify({ tool_preset: toolPreset || null }),
      });
      setSessions((items) =>
        items.map((item) => (item.id === sessionId ? session : item)),
      );
      return session;
    },
    [setSessions],
  );

  const setAgent = useCallback(
    async (sessionId: string, agentId: string) => {
      const session = await api<Session>(`/sessions/${sessionId}/agent`, {
        method: "PUT",
        body: JSON.stringify({ agent_id: agentId }),
      });
      setSessions((items) =>
        items.map((item) => (item.id === sessionId ? session : item)),
      );
      return session;
    },
    [setSessions],
  );
  const sendMessage = useCallback(
    async (sessionId: string, message: string) => {
      // Mark the session as running before awaiting the request. A fast Agent may finish
      // and refresh the session to idle before this POST response arrives.
      const statusUpdatedAt = new Date().toISOString();
      setSessions((items) =>
        items.map((item) =>
          item.id === sessionId
            ? { ...item, status: "running", status_updated_at: statusUpdatedAt }
            : item,
        ),
      );
      try {
        const result = await api<{
          status?: string;
          error?: string;
          title?: string;
        }>(`/sessions/${sessionId}/messages`, {
          method: "POST",
          body: JSON.stringify({ message }),
        });
        if (result.error) throw new Error(result.error);
        // Do not set status here: the completion event may already have changed it to idle.
        if (result.title)
          setSessions((items) =>
            items.map((item) =>
              item.id === sessionId ? { ...item, title: result.title! } : item,
            ),
          );
        return result;
      } catch (error) {
        // Do not replace the session list with an unversioned HTTP snapshot here.
        // The central session hook reconciles authoritative status using
        // status_updated_at; this catch must never reintroduce a stale "running".
        throw error;
      }
    },
    [setSessions],
  );
  const setArchived = useCallback(
    async (sessionId: string, archived: boolean) => {
      const session = await api<Session>(`/sessions/${sessionId}/archive`, {
        method: "PUT",
        body: JSON.stringify({ archived }),
      });
      setSessions((items) =>
        items.map((item) => (item.id === sessionId ? session : item)),
      );
      return session;
    },
    [setSessions],
  );
  const renameSession = useCallback(
    async (sessionId: string, title: string) => {
      const session = await api<Session>(`/sessions/${sessionId}/title`, {
        method: "PUT",
        body: JSON.stringify({ title }),
      });
      setSessions((items) =>
        items.map((item) => (item.id === sessionId ? session : item)),
      );
      return session;
    },
    [setSessions],
  );
  const togglePin = useCallback(
    async (sessionId: string) => {
      const session = await api<Session>(`/sessions/${sessionId}/pin`, {
        method: "PUT",
      });
      setSessions((items) =>
        [...items.map((item) => (item.id === sessionId ? session : item))].sort(
          (a, b) => Number(b.is_pinned) - Number(a.is_pinned),
        ),
      );
      return session;
    },
    [setSessions],
  );
  const deleteSession = useCallback(
    async (sessionId: string) => {
      await api(`/sessions/${sessionId}`, { method: "DELETE" });
      setSessions((items) => items.filter((item) => item.id !== sessionId));
    },
    [setSessions],
  );
  /** Deletes many sessions, keeping whichever the backend refuses (e.g. still running). */
  const deleteSessions = useCallback(
    async (sessionIds: string[]) => {
      const results = await Promise.allSettled(
        sessionIds.map((id) => api(`/sessions/${id}`, { method: "DELETE" })),
      );
      const removed = new Set(
        sessionIds.filter((_, index) => results[index].status === "fulfilled"),
      );
      setSessions((items) => items.filter((item) => !removed.has(item.id)));
      return sessionIds.filter((_, index) => !removed.has(sessionIds[index]));
    },
    [setSessions],
  );
  const interrupt = useCallback(
    async (sessionId: string) => {
      const result = await api<{ status?: string }>(
        `/sessions/${sessionId}/interrupt`,
        { method: "POST" },
      );
      markStatusChanged?.(sessionId);
      const status = result.status || "interrupted";
      const statusUpdatedAt = new Date().toISOString();
      setSessions((items) =>
        items.map((item) =>
          item.id === sessionId
            ? { ...item, status, status_updated_at: statusUpdatedAt }
            : item,
        ),
      );
    },
    [setSessions, markStatusChanged],
  );
  return {
    createSession,
    createPiFork,
    forkFromEntry,
    restartMissingSession,
    setModel,
    setThinking,
    setTools,
    setAgent,
    sendMessage,
    renameSession,
    togglePin,
    setArchived,
    deleteSession,
    deleteSessions,
    interrupt,
  };
}
