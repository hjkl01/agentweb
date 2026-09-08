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

内置 Agent 和用户创建的 Agent 使用统一的 Agent ID 访问。Catalog 是运行时可发现的 Agent Definition，数据库中的 `agents` 保存用户自定义配置以及需要持久化的安装状态。

内置 Definition 当前包括 Codex、Pi、OpenCode、OpenClaw。模型、状态和 Session 接口必须能够解析内置 Definition，即使数据库尚不存在对应记录。

## 3. Agent Adapter 与事件解析

每个 Agent 独立负责启动参数、native session/thread ID、模型发现、模型选择和原生事件映射。

当前结构：

```text
agents/
├── process.rs          # 公共进程生命周期、stdin/stdout/stderr
├── event_parser.rs     # 公共 JSON 字段提取和通用事件映射
├── codex.rs
├── codex_events.rs     # Codex native session / assistant 判断
├── pi.rs
├── pi_events.rs        # Pi message_update 等原生事件
├── opencode.rs
└── openclaw.rs
```

`process.rs` 不再直接实现大段 Agent-specific JSON normalization，而是调用公共 event parser，并将 Codex/Pi 的特殊事件交给对应模块。下一阶段继续把 command/tool/thinking/file 的 provider-specific 规则迁移到各自事件模块。

公共 process 层只负责：启动 Agent CLI、维护运行中进程、读取 stdout/stderr、处理进程退出、interrupt，以及把统一事件交给 EventBus。

## 4. 模型配置与新建对话

模型配置地址不能假设所有 Agent 相同。每个 Agent 使用自己的配置体系或官方 CLI 能力。当前优先适配 Codex 和 Pi；后续 Agent 分别实现独立模型发现器。

```text
New Chat → Agent → GET /api/agents/{agent_id}/models → Model → POST /api/sessions
```

模型属于 Session 配置，也可以通过 `PUT /api/sessions/{id}/model` 切换。模型接口同时支持 DB Agent 和 Catalog 内置 Agent。

## 5. Session

Web Session ID 与 Agent 原生 Session/Thread ID 分离保存。首次运行从 Agent 原生事件获取 native ID；后续消息使用该 ID 恢复原生会话。

## 6. 实时事件

统一事件包括 message、thinking、tool、command、file、agent.error、session 生命周期事件。Session WebSocket 转发所有带 session_id 的 AgentEvent。

## 7. 前端结构

```text
hooks/
├── useSession.ts
├── useSessionEvents.ts
├── useSessionActions.ts
├── useAgentModels.ts
└── useWorkspace.ts

lib/
└── agentActivity.ts

components/
├── ChatPanel.tsx
├── AgentActivity.tsx
├── NewChatDialog.tsx
├── WorkspacePanel.tsx
└── FileTree.tsx
```

Activity 按生命周期合并：thinking.delta 更新同一个 Thinking Activity；Tool/Command output 追加到对应 Activity。

## 8. Workspace / Diff

收到 `file.created`、`file.modified`、`file.deleted` 后通过 revision 触发 Workspace refresh；文件树重新读取后端，Diff 面板打开时重新请求当前 Session diff。

## 9. 文件大小控制

- 每个文件尽量保持单一职责。
- 超过约 300 行时优先继续拆分。
- `process.rs` 不继续堆积 Agent-specific 协议解析。
- GitHub 修改大文件前先分段读取，禁止盲目覆盖。

## 10. Swagger

```text
http://localhost:8080/docs
http://localhost:8080/api-doc/openapi.json
```

所有 HTTP API 都应登记到 OpenAPI。

## 11. 当前演进顺序

1. 统一 Agent Definition / Catalog
2. Codex / Pi 独立模型发现
3. 新建对话 Agent → Model
4. Codex / Pi Session 恢复
5. Agent Activity 生命周期化
6. Workspace / Diff 自动刷新
7. 公共 process 与 provider event parser 拆分
8. OpenCode 独立事件适配
9. Claude Code Adapter
10. OpenClaw 独立事件适配
11. 各 Agent 独立模型发现与配置适配
