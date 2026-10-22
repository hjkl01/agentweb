export type ApiErrorBody = { error: { code: string; message: string } };
export class ApiError extends Error {
  status?: number;
  code?: string;
  path: string;
  constructor(message: string, path: string, status?: number, code?: string) {
    super(message);
    this.name = "ApiError";
    this.path = path;
    this.status = status;
    this.code = code;
  }
}
export type Agent = {
  id: string;
  name: string;
  kind?: string;
  description?: string;
  installed: boolean;
  requirements: string[];
  install_command?: string;
};
export type ScheduledTask = {
  id: string;
  name: string;
  prompt: string;
  agent_id: string;
  working_directory: string;
  model?: string;
  schedule_type: "once" | "interval" | "cron";
  schedule_config: Record<string, unknown>;
  timezone: string;
  session_strategy: "new" | "reuse";
  reuse_session_id?: string;
  enabled: boolean;
  next_run_at?: string;
  last_run_at?: string;
  last_run_status?: string;
  last_run_summary?: string;
  last_run_error?: string;
  run_count: number;
  created_at: string;
  updated_at: string;
};
export type ScheduledTaskRun = {
  id: string;
  task_id: string;
  task_name: string;
  status: string;
  scheduled_at: string;
  started_at?: string;
  finished_at?: string;
  agent_session_id?: string;
  summary?: string;
  error_message?: string;
  created_at: string;
  updated_at: string;
};
export type AgentModel = {
  id: string;
  name: string;
  provider?: string;
  source?: string;
};
export type Session = {
  id: string;
  agent_id: string;
  title: string;
  workspace: string;
  status: string;
  status_updated_at: string;
  native_session_id?: string;
  fork_from_native_session_id?: string;
  model?: string;
  thinking_level?: string;
  tool_preset?: string | null;
  is_pinned: boolean;
  archived_at?: string | null;
  workspace_available?: boolean;
};
export type MessagePart = {
  type: "text" | "thinking" | "toolCall" | "image" | string;
  text?: string;
  name?: string;
  arguments?: unknown;
  tool_call_id?: string;
  mime_type?: string;
  image_index?: number;
};
export type MessageUsage = {
  input?: number;
  output?: number;
  cacheRead?: number;
  cacheWrite?: number;
  totalTokens?: number;
  cost?: { total?: number };
};
export type ChatMessage = {
  id: string;
  role: string;
  content: string;
  created_at?: string;
  parts?: MessagePart[];
  tool_call_id?: string;
  is_error?: boolean;
  usage?: MessageUsage;
  /** toolResult: which tool ran, how long it took, whether Pi cut the output. */
  tool_name?: string;
  duration_ms?: number;
  truncated?: boolean;
  diff?: string;
  /** assistant: answering model, why the turn stopped, provider error. */
  model?: string;
  stop_reason?: string;
  error_message?: string;
};
export type ActivityItem = {
  id: string;
  key: string;
  type: string;
  label: string;
  detail?: string;
};
export type NodeInfo = {
  available: string[];
  installed: string[];
  detected?: { version: string; path: string }[];
  active?: string;
  configured_path?: string;
};
export type FileItem = {
  name: string;
  path: string;
  kind: "file" | "directory" | string;
  size: number;
  status?: "added" | "modified" | "deleted" | "renamed" | "untracked";
};
export type WorkspaceFile = {
  path: string;
  content: string;
  error?: string;
  size?: number;
  binary?: boolean;
  truncated?: boolean;
  message?: string;
  source?: string;
};
export type WorkspaceDiff = {
  status?: string;
  diff?: string;
  truncated?: boolean;
  [key: string]: unknown;
};
export type WorktreeInfo = {
  path: string;
  branch: string | null;
  is_main: boolean;
  available: boolean;
};
export type WorktreesView = {
  is_git: boolean;
  project_root: string | null;
  worktrees: WorktreeInfo[];
};
export type AgentEvent = { type: string; data?: Record<string, any> };
function requestOptions(options?: RequestInit): RequestInit {
  return {
    credentials: "include",
    headers: {
      "Content-Type": "application/json",
      ...(options?.headers || {}),
    },
    ...options,
  };
}
export async function api<T = any>(
  path: string,
  options?: RequestInit,
): Promise<T> {
  const url = "/api" + path;
  let response: Response;
  try {
    response = await fetch(url, requestOptions(options));
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new ApiError(`无法连接后端：${message}`, path);
  }
  const text = await response.text();
  let data: any = {};
  if (text) {
    try {
      data = JSON.parse(text);
    } catch {
      data = {
        error: { code: "INVALID_RESPONSE", message: text.slice(0, 500) },
      };
    }
  }
  if (response.status === 401 && path !== "/auth/login")
    window.dispatchEvent(new CustomEvent("agentweb:auth-expired"));
  if (!response.ok) {
    const error = data?.error;
    const message =
      typeof error === "object"
        ? error.message
        : typeof error === "string"
          ? error
          : data?.message || response.statusText;
    const code = typeof error === "object" ? error.code : undefined;
    throw new ApiError(message, path, response.status, code);
  }
  return data as T;
}
export function fileUrl(sessionId: string, path: string): string {
  return `/sessions/${sessionId}/file/${path.split("/").map(encodeURIComponent).join("/")}`;
}
export type UploadedFile = { path: string; size: number };
export type McpServer = {
  name: string;
  scope?: string;
  source?: string;
  enabled: boolean;
  exposure?: string;
  transport?: string;
  state?: string;
  tools: string[];
  error?: string;
};
export type McpView = { servers: McpServer[]; errors: unknown[] };
export type PiCommand = {
  name: string;
  description?: string;
  source: string;
  scope?: string;
  path?: string;
  origin?: string;
  base_dir?: string;
};
export type PluginPackage = {
  source: string;
  scope: string;
  path?: string;
};
export type SessionSearchHit = {
  session: Session;
  entry_id: string;
  role: string;
  before: string;
  matched: string;
  after: string;
};
export type SessionSearchResponse = {
  results: SessionSearchHit[];
  truncated: boolean;
};
export const THINKING_LEVELS = [
  { value: "off", label: "关闭" },
  { value: "minimal", label: "极简" },
  { value: "low", label: "低" },
  { value: "medium", label: "中" },
  { value: "high", label: "高" },
  { value: "xhigh", label: "极高" },
  { value: "max", label: "最大" },
] as const;

/** 工具预设由后端白名单校验，前端只负责显示它对应的 pi 参数。 */
export const TOOL_PRESETS = [
  {
    value: "default",
    label: "默认",
    hint: "使用 Agent 自身配置的工具",
    args: "",
  },
  {
    value: "readonly",
    label: "只读",
    hint: "禁用 bash / edit / write",
    args: "--tools -bash,-edit,-write",
  },
  {
    value: "nobash",
    label: "禁 bash",
    hint: "只禁用 bash",
    args: "--tools -bash",
  },
  {
    value: "notools",
    label: "无工具",
    hint: "禁用全部工具（含扩展与 MCP）",
    args: "--no-tools",
  },
] as const;
