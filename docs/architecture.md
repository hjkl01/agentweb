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

内置 Definition 当前包括：Codex、Pi、OpenCode、OpenClaw。模型接口对于内置 Agent 不再要求数据库中必须预先存在一行 `agents` 记录。

未来如果需要保存内置 Agent 的安装状态，可以同步写入数据库，但不能依赖“数据库有无记录”判断 Agent 是否属于 Catalog。

## 3. Agent Adapter 原则

不同 Agent 的命令行参数、模型配置、Session 标识和 JSON 事件格式都可能不同。因此不能用一套固定规则解析所有 Agent。

每个 Agent 应独立负责：

- 启动参数
- native session/thread ID
- 模型发现
- 模型选择
- 原生事件解析
- thinking / tool / command / file / message 事件映射

公共层只负责进程生命周期、WebSocket、Session 持久化和统一事件分发。

`process.rs` 只应该逐步收敛为公共进程生命周期和 IO 层；随着 Agent 增加，协议解析应迁移到各自的 adapter/event 模块，避免形成巨型条件分支文件。

## 4. 模型配置与新建对话

模型配置地址不能假设所有 Agent 相同。每个 Agent 必须使用自己的模型配置或官方 CLI 能力。

当前优先适配：

- **Codex**：读取 Codex 自己的配置体系。
- **Pi**：通过 Pi 自己的模型注册/CLI 能力发现模型，避免复制 Provider 注册表和认证逻辑。

后续新增 OpenCode、Claude Code、OpenClaw 时，应分别增加对应的模型发现器，不应复用 Codex/Pi 的配置路径。

新建对话流程：

```text
New Chat
   │
   ▼
选择已安装 Agent
   │
   ▼
GET /api/agents/{agent_id}/models
   │
   ▼
显示该 Agent 可用模型
   │
   ├── 选择具体模型 ──► POST /api/sessions { agent_id, model }
   │
   └── 无可发现模型 ──► 使用 Agent 默认模型
```

模型选择属于 Session 配置，而不是全局 Agent 配置。用户也可以在已经创建的会话中通过 `PUT /api/sessions/{id}/model` 切换模型。

模型接口必须同时支持：

1. 数据库中的用户自定义 Agent；
2. Catalog 中的内置 Agent。

不能因为 Codex/Pi 没有数据库记录而返回 404。

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

首次运行时从 Agent 原生事件中获取 native ID；后续消息使用该 ID 恢复原生会话。

禁止使用普通 message/tool 对象中的任意 `id` 覆盖已经保存的 native ID。

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

Session WebSocket 必须转发所有带 `session_id` 的 AgentEvent，包括 thinking、tool、command 和 file 事件；安装事件不属于 Session WebSocket。

前端收到 file 事件后应刷新 Workspace 文件树，并在 Diff 面板打开时重新获取当前 Session 的 diff。

## 7. 前端结构

前端按职责拆分：

```text
hooks/
├── useSession.ts          # Session 组合层
├── useSessionEvents.ts    # WebSocket 生命周期
├── useSessionMessages.ts  # 消息与流式内容
├── useSessionActions.ts   # 创建/删除/中断/模型
├── useAgentModels.ts      # Agent 模型发现
└── useWorkspace.ts        # 文件树与 Diff

components/
├── ChatPanel.tsx
├── AgentActivity.tsx
├── NewChatDialog.tsx
├── WorkspacePanel.tsx
└── FileTree.tsx
```

`useSession.ts` 不应继续承担所有事件解析和 Workspace 联动逻辑。

Agent Activity 中 thinking/tool/command 应按生命周期合并，而不是每一个 delta 创建一条新记录。例如连续 `thinking.delta` 应更新同一个 Thinking Activity；command/tool 的 output 也应追加到对应 Activity。

## 8. Workspace / Diff

收到 `file.created`、`file.modified`、`file.deleted` 后，前端应刷新当前 Workspace 文件树。

如果用户打开 Diff 面板，应重新请求当前 Session 的 diff，而不是依赖 Agent 原始事件中的文件内容。

```text
Agent file event
      │
      ▼
Session event hook
      │
      ▼
Workspace refresh
      ├── File Tree
      └── Diff
```

## 9. 文件大小控制

为了便于维护和避免自动代码处理时单文件过大，项目代码应按职责拆分：

- 公共 process 层不解析所有 Agent 协议。
- 每个 Agent 使用独立 adapter / command / event / model 模块。
- 前端 Chat、Activity、Workspace、FileTree 分离。
- 每个文件尽量保持单一职责；超过约 300 行时优先继续拆分。
- GitHub 自动修改大文件时必须先分段读取，避免盲目覆盖。

## 10. Swagger

后端统一提供 OpenAPI/Swagger UI：

```text
http://localhost:8080/docs
```

OpenAPI JSON：

```text
http://localhost:8080/api-doc/openapi.json
```

所有 HTTP API 都应在 OpenAPI 中登记；WebSocket endpoint 也登记其用途和路径。

## 11. 开发环境 WebSocket

`make dev` 同时启动 Rust 后端和 Vite。Vite 将 `/api` 代理到 `127.0.0.1:8080`，并启用 WebSocket 代理。

如果浏览器在后端刚启动、重编译或关闭页面时断开 WebSocket，Vite 可能打印 `write EPIPE` / `ws proxy socket error`。这通常表示代理向已经关闭的 socket 写数据，不等同于 Rust 编译失败。

真正需要关注的是后端是否仍运行、`GET /api/health` 是否正常，以及 `/api/sessions/{id}/events` 是否能保持连接。如果 EPIPE 持续出现且后端同时退出，再检查后端日志和 WebSocket 生命周期。

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
