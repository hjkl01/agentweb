import { useMemo, useState } from "react";
import { useAutoScroll } from "./hooks/useAutoScroll";
import { useAppController } from "./hooks/useAppController";
import { Sidebar } from "./components/Sidebar";
import { ChatPanel } from "./components/ChatPanel";
import { TerminalPanel } from "./components/TerminalPanel";
import { WorkspacePanel } from "./components/WorkspacePanel";
import { AgentCatalog } from "./components/AgentCatalog";
import { SettingsDialog } from "./components/SettingsDialog";
import { ChatSelectionDialog } from "./components/ChatSelectionDialog";
import { UserPage } from "./components/UserPage";
import { ScheduledTasksPanel } from "./components/ScheduledTasksPanel";
import { useCompletionSound, usePreferences } from "./hooks/usePreferences";
import type { Agent } from "./types";
import "./styles/account.css";
type Props = {
  sessionId?: string;
  navigate: (path: string, replace?: boolean) => void;
};
export function App({ sessionId, navigate }: Props) {
  const c = useAppController(sessionId, navigate);
  const [configAgent, setConfigAgent] = useState<Agent>();
  const [terminalOpen, setTerminalOpen] = useState(false);
  const [scheduledTasksOpen, setScheduledTasksOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const { preferences, update, testSound } = usePreferences();
  const runningIds = useMemo(
    () =>
      c.session.sessions
        .filter((session) => session.status === "running")
        .map((session) => session.id)
        .join(","),
    [c.session.sessions],
  );
  useCompletionSound(preferences.completionSound, runningIds);
  const activityVersion = useMemo(
    () =>
      c.session.activity
        .map((x) => `${x.key}:${x.type}:${x.detail?.length || 0}`)
        .join("|"),
    [c.session.activity],
  );
  useAutoScroll(c.chatRef, {
    contentVersion: `${c.session.messages.length}:${c.session.stream.length}:${activityVersion}:${sessionId || ""}`,
    force: c.current?.status === "running",
  });
  const openNewChat = () => {
    // Workspace selection is mandatory even when default Agent/model settings exist.
    c.newChatInWorkspace();
  };
  return (
    <div
      className="app"
      onClick={() => c.sessionMenu && c.setSessionMenu(undefined)}
    >
      {c.startupError && (
        <div className="startup-error" role="alert">
          <strong>连接或运行异常</strong>
          <span>{c.startupError}</span>
          <button
            onClick={() => {
              c.runtime.refresh();
              c.session.refreshSessions();
              c.workspace.refresh();
            }}
          >
            重试
          </button>
        </div>
      )}
      <Sidebar
        agents={c.runtime.agents}
        sessions={c.session.sessions}
        active={sessionId}
        sessionMenu={c.sessionMenu}
        onNewChat={openNewChat}
        onSelectSession={(id) => navigate(`/sessions/${id}`)}
        onSessionMenu={(id) =>
          c.setSessionMenu((v) => (v === id ? undefined : id))
        }
        onRenameSession={c.renameSession}
        onTogglePin={c.togglePin}
        onToggleArchive={c.toggleArchive}
        onDeleteSession={c.deleteSession}
        onDeleteSessions={c.deleteSessions}
        onNewChatInWorkspace={c.newChatInWorkspace}
        onOpenCatalog={() => c.setCatalogOpen(true)}
        onOpenAccount={() => c.setAccountOpen(true)}
        onLogout={c.logout}
        onHome={() => navigate("/")}
        onOpenTerminal={() => setTerminalOpen(true)}
        onOpenScheduledTasks={() => setScheduledTasksOpen(true)}
        onOpenSettings={() => setSettingsOpen(true)}
      />
      {terminalOpen ? (
        <TerminalPanel
          sessionId={sessionId}
          onClose={() => setTerminalOpen(false)}
        />
      ) : (
        <ChatPanel
          current={c.current}
          currentAgent={c.currentAgent}
          agents={c.runtime.agents}
          active={sessionId}
          models={c.modelState.models}
          modelLoading={c.modelState.loading}
          messages={c.session.messages}
          stream={c.session.stream}
          activity={c.session.activity}
          activityOpen={c.session.activityOpen}
          input={c.input}
          queuedCount={c.queuedMessages.length}
          pendingUser={c.session.pendingUser}
          preferences={preferences}
          textareaRef={c.textareaRef}
          chatRef={c.chatRef}
          onInputChange={c.setInput}
          onAgentChange={c.changeAgent}
          onModelChange={c.changeModel}
          onThinkingChange={c.changeThinking}
          onToolsChange={c.changeTools}
          onSend={c.send}
          onToggleActivity={() => c.session.setActivityOpen((v) => !v)}
          onStop={c.stop}
          onRestartMissingSession={c.restartMissingSession}
          onForkSession={c.forkSession}
          onForkFromMessage={c.forkFromMessage}
          onNewChat={openNewChat}
          onOpenSettings={() => setSettingsOpen(true)}
          onAutoResize={c.autoResize}
        />
      )}
      <ScheduledTasksPanel
        open={scheduledTasksOpen}
        agents={c.runtime.agents}
        onClose={() => setScheduledTasksOpen(false)}
      />
      <WorkspacePanel
        current={c.current}
        files={c.workspace.files}
        fileFilter={c.workspace.filter}
        selectedFile={c.workspace.selectedFile}
        selectedLine={c.focusLine}
        diff={c.workspace.diff}
        filesError={c.workspace.error}
        tab={c.workspace.tab}
        onFilterChange={c.workspace.setFilter}
        onSelectTab={c.workspace.selectTab}
        onRefresh={c.workspace.refresh}
        onOpenFile={c.openWorkspaceFile}
        onCloseFile={() => {
          c.workspace.closeFile();
          c.setFocusLine(undefined);
        }}
      />
      <SettingsDialog
        open={settingsOpen}
        agents={c.runtime.agents}
        agent={configAgent}
        workspace={c.current?.workspace}
        preferences={preferences}
        onUpdate={update}
        onTestSound={testSound}
        onClose={() => {
          setSettingsOpen(false);
          setConfigAgent(undefined);
        }}
      />
      <ChatSelectionDialog
        open={c.newChatOpen}
        agents={c.runtime.agents}
        onClose={() => c.setNewChatOpen(false)}
        onCreate={c.createChat}
        defaultAgentId={c.runtime.defaultAgentId}
        defaultModel={c.runtime.defaultModel}
        defaultWorkspace={c.newChatWorkspace}
        onDefaultChange={c.runtime.saveDefaults}
        onManageAgents={() => {
          c.setNewChatOpen(false);
          c.setCatalogOpen(true);
        }}
      />
      <AgentCatalog
        open={c.catalogOpen}
        agents={c.runtime.agents}
        node={c.runtime.node}
        nodeVersion={c.runtime.nodeVersion}
        installingNode={c.installation.installingNode}
        installingAgent={c.installation.installingAgent}
        nodeInstallFeedback={c.installation.nodeInstallFeedback}
        agentInstallFeedback={c.installation.agentInstallFeedback}
        defaultAgentId={c.runtime.defaultAgentId}
        defaultModel={c.runtime.defaultModel}
        onDefaultChange={c.runtime.saveDefaults}
        onClose={() => c.setCatalogOpen(false)}
        onNodeVersionChange={c.runtime.setNodeVersion}
        onInstallNode={c.installation.installNode}
        onInstallAgent={c.installation.installAgent}
        onInstallCustomAgent={c.installation.installCustomAgent}
        onConfigureAgent={(agent) => {
          setConfigAgent(agent);
          setSettingsOpen(true);
        }}
      />
      <UserPage
        open={c.accountOpen}
        onClose={() => c.setAccountOpen(false)}
        onLogout={c.logout}
      />
    </div>
  );
}
