# Agent Web 架构方案

## 1. 总体定位

Agent Web 是 Agent Runtime 的 Web 交互层，不负责替代 Codex、Pi、OpenCode、Claude Code、OpenClaw 等 Agent 的推理和工具执行。

```text
Browser
   │ HTTP / WebSocket
   ▼
Agent Web Backend
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

## 3. Agent Adapter 与事件解析

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

`process.rs` 不保存数据库 Session 编排逻辑，也不应该继续堆积 provider-specific 协议。`session.rs` 负责读取 Agent/Session 配置、调用对应 Adapter、保存 native session ID 和最终 assistant message。

## 4. 模型配置与新建对话

模型配置地址不能假设所有 Agent 相同。每个 Agent 使用自己的配置体系或官方 CLI 能力。

当前优先适配 Codex 和 Pi：

- Codex：读取 `CODEX_HOME/config.toml` 或 `~/.codex/config.toml` 中的模型配置。
- Pi：通过 `pi --list-models` 获取 Agent 自己的 Model Registry，避免复制 Pi 的 provider/model 配置。

Pi 的模型配置本身还支持 `~/.pi/agent/models.json`，因此后端不应自行假设 provider、API URL 或认证方式。citeturn1search1turn1search4

Codex 当前没有可依赖的 `codex models` 官方命令，因此暂时以其配置文件为模型发现来源，而不是伪造一个统一模型列表。citeturn1search5

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

模型属于 Session 配置，也可以通过 `PUT /api/sessions/{id}/model` 切换。

## 5. Session / Thread

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

首次运行从 Agent 原生事件获取 native ID；后续消息由对应 Adapter 使用该 ID 恢复原生会话。最终 assistant 文本同时持久化到 `messages`，因此刷新页面不会丢失已经完成的回答。

## 6. 实时事件

统一事件包括 message、thinking、tool、command、file、error、session 生命周期事件。Session WebSocket 只转发对应 `session_id` 的事件。

## 7. 前端结构

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

## 8. Workspace / Diff

Workspace API 对外使用 workspace-relative path，不再把服务器绝对路径直接暴露给前端。

读取文件时：

1. canonicalize workspace root；
2. canonicalize 请求文件；
3. 检查请求文件是否仍位于 workspace root；
4. 不允许绝对路径、`../` 穿越或通过 symlink 跳出 workspace。

收到 `file.created`、`file.modified`、`file.deleted` 后通过 revision 触发 Workspace refresh；Diff 面板打开时重新请求当前 Session diff。

## 9. Agent 安装

Agent 安装属于 Catalog/Runtime 层，而不是 Agent Adapter。

Codex 当前使用官方 npm 包 `@openai/codex` 安装，安装完成后再检测 `codex --version`。官方同时提供独立安装脚本和 npm 安装方式。citeturn0search0

Node.js Agent 则统一使用受控 Node runtime 的 npm 安装路径，避免依赖宿主机全局 Node。

## 10. 文件大小控制

- 每个文件尽量保持单一职责。
- 超过约 300 行时优先继续拆分。
- `process.rs` 只处理公共进程生命周期。
- Agent-specific 事件进入各自模块。
- GitHub 修改大文件前先分段读取，禁止盲目覆盖。

## 11. Swagger

```text
http://localhost:8080/docs
http://localhost:8080/api-doc/openapi.json
```

所有 HTTP API 都应登记到 OpenAPI；WebSocket endpoint 同样记录用途和路径。

## 12. 当前演进顺序

1. 统一 Agent Definition / Catalog
2. Codex / Pi 独立模型发现
3. 新建对话 Agent → Model
4. Codex / Pi Session 恢复
5. Agent Activity 生命周期化
6. Workspace / Diff 自动刷新
7. 公共 process 与 provider event parser 拆分
8. Codex / Pi 安装与运行时隔离
9. OpenCode 独立事件适配
10. Claude Code Adapter
11. OpenClaw 独立事件适配
12. 各 Agent 独立模型发现与配置适配