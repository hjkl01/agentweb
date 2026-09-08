export type Agent = {
  id: string;
  name: string;
  description?: string;
  installed: boolean;
  requirements: string[];
};

export type Session = {
  id: string;
  agent_id: string;
  title: string;
  workspace: string;
  status: string;
  native_session_id?: string;
};

export type ChatMessage = {
  id: string;
  role: string;
  content: string;
  created_at?: string;
};

export type ActivityItem = {
  id: string;
  type: string;
  label: string;
  detail?: string;
};

export type NodeInfo = {
  available: string[];
  installed: string[];
  active?: string;
};

export type FileItem = {
  path: string;
  kind: string;
};

export type WorkspaceFile = {
  path: string;
  content: string;
};

export type WorkspaceDiff = {
  diff?: string;
  [key: string]: unknown;
};

export type AgentEvent = {
  type: string;
  data?: Record<string, any>;
};