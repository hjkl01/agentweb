# Agent Web 架构方案

## 1. 总体定位

Agent Web 是 Agent Runtime 的 Web 交互层，不负责替代 Codex、Pi、OpenCode、Claude Code、OpenClaw 等 Agent 的推理和工具执行。

```text
Browser
   │ HTTP / WebSocket
   ▼
Agent Web Backend
   ├── Basic Auth / User Store
   ├── Agent Registry / Catalog
   ├── Session / SQLite
   ├── Workspace / Diff
   └── Agent Manager
          ├── Codex Adapter + Events
          ├── Pi Adapter + Events
          ├── OpenCode Adapter
          ├── OpenClaw Adapter
          └── Generic Adapter
                  │
                  ▼
             Agent CLI
```

## 2. Agent Definition / Registry

内置 Agent 和用户创建的 Agent 使用统一的 Agent ID。Catalog 负责运行时发现，SQLite `agents` 保存配置和安装状态。初始化数据库时会写入 Codex、Claude Code、Pi、OpenCode、OpenClaw 的内置记录，但使用 `INSERT OR IGNORE`，不会覆盖用户修改。

## 3. Backend HTTP 分层

HTTP handler 按职责拆分：

```text
api/
├── mod.rs          # 对外 handler re-export
├── agents.rs       # Agent CRUD / status / models
├── runtime.rs      # Node / Agent runtime / installation
├── sessions.rs     # Session / messages / WebSocket
├── workspace.rs    # File tree / file content / diff
└── health.rs       # Health check
```

`/api/health` 保持公开，用于 Docker healthcheck；其余 `/api/*` 接口使用 HTTP Basic Authentication。首次初始化数据库时生成一次性 admin 随机密码并只写入服务日志。

## 4. Agent Adapter 与事件解析

每个 Agent 独立负责启动参数、native session/thread ID、模型发现、模型选择和原生事件映射。

```text
agents/
├── process.rs          # 公共进程生命周期、stdout/stderr、interrupt
├── session.rs          # Session → AgentConfig → Adapter 的编排
├── event_parser.rs     # 公共 JSON 字段提取和通用事件映射
├── codex.rs
├── codex_events.rs
├── pi.rs
├── pi_events.rs
├── opencode.rs
└── openclaw.rs
```

`process.rs` 负责公共进程生命周期；Linux 下 Agent 进程通过 `setsid` 建立独立 process group，Stop/Delete 会同时终止整个进程组，避免 Agent 派生的 shell/python/git 等子进程残留。

## 5. 模型配置与新建对话

模型配置地址不能假设所有 Agent 相同。每个 Agent 使用自己的配置体系或官方 CLI 能力。

当前优先适配 Codex 和 Pi：

- Codex：读取 `CODEX_HOME/config.toml` 或 `~/.codex/config.toml` 中的模型配置。
- Pi：通过 `pi --list-models` 获取 Agent 自己的 Model Registry；Pi 配置还支持 `~/.pi/agent/models.json`。

Codex 当前没有可依赖的统一 `codex models` 命令，因此暂时以配置文件为模型发现来源。

```text
New Chat
   ↓
Agent
   ↓
GET /api/agents/{agent_id}/models
   ↓
Model
   ↓
POST /api/sessions
```

模型属于 Session 配置，也可以通过 `PUT /api/sessions/{id}/model` 切换。运行中的 Session 不允许切换模型或重复发送消息。

## 6. Session / Thread

Web Session ID 与 Agent 原生 Session/Thread ID 分离保存。

```text
Web Session
 ├── session.id
 ├── agent_id
 ├── model
 └── native_session_id
          ↓
      Agent Adapter
          ↓
   Codex thread / Pi session / ...
```

首次运行从 Agent 原生事件获取 native ID；后续消息由对应 Adapter 使用该 ID 恢复原生会话。最终 assistant 文本同时持久化到 `messages`，刷新页面不会丢失已经完成的回答。

Session workspace 默认由后端创建在 `AGENTWEB_WORKSPACE_DIR/<session_id>`。请求 workspace 只能是根目录下的安全相对路径，不能使用绝对路径、`..`、Root 或 Windows Prefix 穿越。Git HEAD fallback 使用同一套路径校验。

Agent 启动时使用 Runtime 层解析出的真实 executable 路径，而不是仅依赖 PATH 中的命令名；因此用户配置的独立 Agent 路径和 Node runtime 安装路径在实际运行时保持一致。

## 7. 实时事件

统一事件包括 message、thinking、tool、command、file、error、session 生命周期事件。Session WebSocket 只转发对应 `session_id` 的事件。

当前 WebSocket 使用内存 broadcast，因此只保证实时传输；已完成 assistant message 持久化到 SQLite。重新打开 Session 时前端先读取 `/messages` 再建立 WebSocket。

## 8. 前端结构

```text
hooks/
├── useSession.ts
├── useSessionEvents.ts
├── useSessionActions.ts
├── useAgentModels.ts
└── useWorkspace.ts

components/
├── ChatPanel.tsx
├── AgentActivity.tsx
├── NewChatDialog.tsx
├── WorkspacePanel.tsx
└── FileTree.tsx
```

Activity 按生命周期合并：thinking.delta 更新同一个 Thinking Activity；Tool/Command output 追加到对应 Activity。

## 9. Workspace / Diff

Workspace API 对外使用 workspace-relative path，不把服务器绝对路径直接暴露给前端。

读取文件时：

1. canonicalize workspace root；
2. 校验安全相对路径；
3. canonicalize 请求文件；
4. 检查请求文件仍位于 workspace root；
5. 不允许 symlink 跳出 workspace。

收到 `file.created`、`file.modified`、`file.deleted` 后通过 revision 触发 Workspace refresh；Diff 面板打开时重新请求当前 Session diff。文件预览限制 512 KiB，单文件 diff 限制 1 MiB，总 diff 限制 4 MiB，并跳过常见依赖/构建目录。

## 10. Agent 安装

Agent 安装属于 Catalog/Runtime 层，而不是 Agent Adapter。

Codex 使用 `@openai/codex` 安装；Node.js Agent 统一使用受控 Node runtime 的 npm 安装路径。Codex、Claude Code、OpenCode、Pi 都明确声明 Node.js requirement。

安装完成后重新执行 Agent 的 `--version` 检查，并更新 SQLite 中的 installed/version 状态。运行 Agent 时再次解析真实 executable，避免“Catalog 显示已安装但实际进程找不到”的问题。

## 11. 本地开发与 Docker

本地开发：

```text
make install
        ↓
make dev
├── frontend: Vite :5173
└── backend:  Rust :8080
       ├── ./data
       ├── ./workspaces
       └── ./runtimes
```

`make dev` 只负责启动，不再每次重复 `npm install` / `cargo fetch`；依赖安装单独由 `make install` 完成，并使用 `npm ci` 锁定 `package-lock.json`。

Docker：

```text
make docker-up
        ↓
 docker compose
        ↓
 agentweb :8080
   ├── /data → ./data
   ├── /workspaces → ./workspaces
   └── /opt/agent-runtimes → ./runtimes
```

Docker 使用 `init: true`、健康检查、固定 package-lock 安装和 `util-linux`（提供 `setsid`）。最终镜像不预装所有 Agent；Node/runtime 与 Agent 按需安装。

前端是 SPA，后端静态服务对不存在的前端路径回退到 `index.html`，因此直接打开或刷新 `/sessions/{session_id}` 可以正常恢复 Session 页面。

## 12. 数据库升级

SQLite schema 使用 `schema_meta` 记录版本。初始化时先确保基础表存在，再通过 `PRAGMA table_info` 检查新增字段；旧数据库会幂等补齐 `native_session_id`、`model` 等字段，不再依赖忽略 `ALTER TABLE` 错误。

## 13. 文件大小与代码组织

- 每个文件尽量保持单一职责。
- 超过约 300 行时优先继续拆分。
- `process.rs` 只处理公共进程生命周期。
- Agent-specific 事件进入各自模块。
- HTTP handler 按 agents/runtime/sessions/workspace 分模块。
- GitHub 修改大文件前先分段读取，禁止盲目覆盖。

## 14. Swagger

```text
http://localhost:8080/docs
http://localhost:8080/api-doc/openapi.json
```

所有 HTTP API 都登记到 OpenAPI；WebSocket endpoint 同样记录用途和路径。API 调试请求需要使用 admin Basic Auth。

## 15. 当前演进顺序

1. 统一 Agent Definition / Catalog
2. Codex / Pi 独立模型发现
3. 新建对话 Agent → Model
4. Workspace 生命周期与隔离
5. Codex / Pi Session 恢复
6. Agent Activity 生命周期化
7. Workspace / Diff 自动刷新
8. Backend API 按职责拆分
9. Codex / Pi 安装与运行时隔离
10. Docker / Session SPA / Runtime / Security hardening
11. OpenCode 独立事件适配
12. Claude Code Adapter
13. OpenClaw 独立事件适配
14. 各 Agent 独立模型发现与配置适配
15. 持久化 streaming message / WebSocket resume
