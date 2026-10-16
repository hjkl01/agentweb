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
      setSessions((items) =>
        items.map((item) =>
          item.id === sessionId ? { ...item, status: "running" } : item,
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
        // Reconcile optimistic state if the request failed or the session was already running.
        try {
          setSessions(await api<Session[]>("/sessions"));
        } catch {
          /* preserve the original error */
        }
        throw error;
      }
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
  const interrupt = useCallback(
    async (sessionId: string) => {
      const result = await api<{ status?: string }>(
        `/sessions/${sessionId}/interrupt`,
        { method: "POST" },
      );
      markStatusChanged?.(sessionId);
      setSessions((items) =>
        items.map((item) =>
          item.id === sessionId
            ? { ...item, status: result.status || "interrupted" }
            : item,
        ),
      );
    },
    [setSessions, markStatusChanged],
  );
  return {
    createSession,
    createPiFork,
    restartMissingSession,
    setModel,
    setAgent,
    sendMessage,
    renameSession,
    togglePin,
    deleteSession,
    interrupt,
  };
}
