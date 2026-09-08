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
          ├── Codex Adapter
          ├── Pi Adapter
          ├── OpenCode Adapter
          ├── OpenClaw Adapter
          └── Generic Adapter
                  │
                  ▼
             Agent CLI
```

## 2. Agent Definition / Registry

内置 Agent 和用户创建的 Agent 不能使用两套身份体系。Agent Catalog 是运行时可发现的 Agent Definition，数据库中的 `agents` 用于保存用户自定义配置；访问模型、状态和 Session 时必须能够通过同一个 Agent ID 找到 Definition。

内置 Definition 当前包括 Codex、Pi、OpenCode、OpenClaw。模型接口对于内置 Agent 不再要求数据库中必须预先存在一行 `agents` 记录。

未来如果需要保存内置 Agent 的安装状态，可以同步写入数据库，但不能依赖“数据库有无记录”判断 Agent 是否属于 Catalog。

## 3. Agent Adapter 原则

不同 Agent 的命令行参数、模型配置、Session 标识和 JSON 事件格式都可能不同。因此不能用一套固定规则解析所有 Agent。

每个 Agent 应独立负责：启动参数、native session/thread ID、模型发现、模型选择、原生事件解析，以及 thinking/tool/command/file/message 事件映射。

公共层只负责进程生命周期、WebSocket、Session 持久化和统一事件分发。`process.rs` 只应该逐步收敛为公共进程生命周期和 IO 层；随着 Agent 增加，协议解析应迁移到各自的 adapter/event 模块。

## 4. 模型配置与新建对话

模型配置地址不能假设所有 Agent 相同。每个 Agent 必须使用自己的模型配置或官方 CLI 能力。

当前优先适配：

- **Codex**：读取 Codex 自己的配置体系。
- **Pi**：通过 Pi 自己的模型注册/CLI 能力发现模型，避免复制 Provider 注册表和认证逻辑。

后续 OpenCode、Claude Code、OpenClaw 必须分别增加对应模型发现器，不复用 Codex/Pi 的配置路径。

新建对话流程：

```text
New Chat → 选择已安装 Agent → GET /api/agents/{agent_id}/models
                                      ↓
                              显示 Agent 模型列表
                                      ↓
                         POST /api/sessions { model }
```

模型选择属于 Session 配置。已创建会话可以通过 `PUT /api/sessions/{id}/model` 切换模型。

模型接口同时支持数据库中的用户自定义 Agent 和 Catalog 中的内置 Agent，不能因为 Codex/Pi 没有数据库记录而返回 404。

## 5. Session 设计

Agent Web Session ID 与 Agent 原生 Session/Thread ID 分离保存：

```text
Web Session
├── id
├── agent_id
├── model
├── workspace
└── native_session_id
```

首次运行时从 Agent 原生事件中获取 native ID；后续消息使用该 ID 恢复原生会话。禁止使用普通 message/tool 对象中的任意 `id` 覆盖已经保存的 native ID。

## 6. 实时事件设计

Agent 原始事件先由对应 Adapter 解析，再转换为统一 AgentEvent：

```text
message.started / delta / completed
thinking.started / delta / completed
tool.started / output / completed
command.started / output / completed
file.created / modified / deleted
agent.error
session.started / completed
```

Session WebSocket 必须转发所有带 `session_id` 的 AgentEvent。前端收到 file 事件后通过 revision 触发 Workspace refresh；文件树始终重新读取后端，Diff 面板打开时重新请求当前 Session diff。

## 7. 前端结构

前端按职责拆分：

```text
hooks/
├── useSession.ts          # Session 组合层
├── useSessionEvents.ts    # WebSocket 与事件分发
├── useSessionActions.ts   # 创建/删除/中断/模型
├── useAgentModels.ts      # Agent 模型发现
└── useWorkspace.ts        # 文件树与 Diff

lib/
└── agentActivity.ts       # Activity 生命周期聚合

components/
├── ChatPanel.tsx
├── AgentActivity.tsx
├── NewChatDialog.tsx
├── WorkspacePanel.tsx
└── FileTree.tsx
```

`useSession.ts` 只负责组合状态和 Hook，不再承担 WebSocket、事件解析和 Action 实现。

Agent Activity 中 thinking/tool/command 按生命周期合并，而不是每个 delta 创建新记录。连续 `thinking.delta` 更新同一个 Activity；tool/command output 追加到对应 Activity。

## 8. Workspace / Diff

收到 `file.created`、`file.modified`、`file.deleted` 后：

```text
Agent file event
      ↓
Session event hook
      ↓
workspaceRevision + 1
      ↓
useWorkspace refresh
   ├── File Tree
   └── Diff（当前 tab 为 diff 时）
```

因此 Agent 修改文件后，用户无需手动刷新即可看到最新文件树和 Diff。

## 9. 文件大小控制

为了便于维护和避免自动代码处理时单文件过大：

- 公共 process 层不解析所有 Agent 协议。
- 每个 Agent 使用独立 adapter / command / event / model 模块。
- 前端 Chat、Activity、Workspace、FileTree 分离。
- 每个文件尽量保持单一职责；超过约 300 行时优先继续拆分。
- GitHub 自动修改大文件时先分段读取，避免盲目覆盖。

## 10. Swagger

后端统一提供 OpenAPI/Swagger UI：

```text
http://localhost:8080/docs
http://localhost:8080/api-doc/openapi.json
```

所有 HTTP API 都应在 OpenAPI 中登记；WebSocket endpoint 也登记用途和路径。

## 11. 开发环境 WebSocket

`make dev` 同时启动 Rust 后端和 Vite。Vite 将 `/api` 代理到 `127.0.0.1:8080` 并启用 WebSocket 代理。

浏览器在后端重启、重编译或关闭页面时断开 WebSocket，Vite 可能打印 `write EPIPE`。真正需要关注的是后端是否仍运行、`GET /api/health` 是否正常，以及 `/api/sessions/{id}/events` 是否能保持连接。

## 12. 当前演进顺序

1. 统一 Agent Definition / Catalog 与用户自定义 Agent 的访问模型
2. Codex / Pi 独立模型发现
3. 新建对话时按 Agent 选择模型
4. Codex / Pi 原生 Session 恢复
5. 前端 Agent Activity 生命周期化
6. 文件修改后 Workspace / Diff 自动刷新
7. 拆分公共 process 与 Agent-specific event parser
8. 独立 OpenCode Adapter
9. 独立 Claude Code Adapter
10. 独立 OpenClaw Adapter
11. 各 Agent 独立模型发现与配置适配
