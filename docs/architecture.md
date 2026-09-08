# Agent Web 架构方案

## 1. 总体定位

Agent Web 是 Agent Runtime 的 Web 交互层，不负责替代 Codex、Pi、OpenCode、Claude Code、OpenClaw 等 Agent 的推理和工具执行。

```text
Browser
   │ HTTP / WebSocket
   ▼
Agent Web Backend
   ├── Session / SQLite
   ├── Agent Manager
   ├── Workspace / Diff
   └── Agent Adapter
          ├── Codex Adapter
          ├── Pi Adapter
          ├── OpenCode Adapter
          ├── OpenClaw Adapter
          └── Generic Adapter
                  │
                  ▼
             Agent CLI
```

## 2. Agent Adapter 原则

不同 Agent 的命令行参数、模型配置、Session 标识和 JSON 事件格式都可能不同。因此不能用一套固定规则解析所有 Agent。

每个 Agent 应独立负责：

- 启动参数
- native session/thread ID
- 模型发现
- 模型选择
- 原生事件解析
- thinking / tool / command / file / message 事件映射

公共层只负责进程生命周期、WebSocket、Session 持久化和统一事件分发。

## 3. 模型配置原则

模型配置地址不能假设所有 Agent 相同。

当前优先适配：

- **Codex**：读取 Codex 自己的配置体系；同时保留 Agent CLI 自身行为作为最终依据。
- **Pi**：通过 Pi 自己的模型注册/CLI 能力发现模型，避免复制 Provider 注册表和认证逻辑。

后续新增 OpenCode、Claude Code、OpenClaw 时，应分别增加对应的模型发现器，不应复用 Codex/Pi 的配置路径。

前端显示：

```text
Provider / Model
```

如果 Agent 没有可发现的模型，则显示 `Agent 默认模型`，表示模型选择权仍由 Agent 自己决定。

## 4. Session 设计

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

## 5. 实时事件设计

Agent 原始事件先由对应 Adapter 解析，再转换为统一 AgentEvent：

```text
message.started
message.delta
message.completed

thinking.started
thinking.delta
thinking.completed

tool.started
tool.output
tool.completed

command.started
command.output
command.completed

file.created
file.modified
file.deleted

agent.error
session.started
session.completed
```

这样前端无需知道 Codex、Pi 的原始 JSON 格式。

### Pi

Pi JSON 模式中的 `message_update` 以及 `assistantMessageEvent` 应由 Pi Adapter 处理；`text_delta` 映射到 assistant message，`thinking_delta` 映射到 thinking。

Pi tool execution 的 start/update/end 事件映射到统一 tool 生命周期。

### Codex

Codex JSON 输出中的 thread/item 生命周期由 Codex Adapter 解析。command execution、MCP/tool call、assistant message 等事件映射到统一事件。

### 后续 Agent

OpenCode、Claude Code、OpenClaw 必须使用各自 Adapter 解析，不应继续向一个巨大的 `process.rs` 增加越来越多的 Agent 特判。

## 6. 前端事件展示

前端将统一事件展示成 Agent activity：

```text
Agent activity

✓ Thinking
  分析项目结构...

✓ bash
  $ cargo test
  Compiling ...
  test result: ok

✓ edit
  backend/src/api.rs

● Running
  npm run build
```

Activity UI 与聊天消息、Workspace 文件树分离。聊天区域负责最终回答，Activity 负责执行过程，Workspace 负责文件状态。

## 7. Workspace / Diff

收到 `file.created`、`file.modified`、`file.deleted` 后，前端应刷新当前 Workspace 文件树。

如果用户打开 Diff 面板，应重新请求当前 Session 的 diff，而不是依赖 Agent 原始事件中的文件内容。

因此：

```text
Agent file event
      │
      ▼
refresh workspace
      │
      ├── File Tree
      └── Diff
```

## 8. 文件大小控制

为了便于维护和避免自动代码处理时单文件过大，项目代码应按职责拆分：

- `process.rs`：进程生命周期和公共 IO，不承担所有 Agent 的业务解析。
- `models.rs`：模型发现。
- 每个 Agent 使用独立 adapter 文件。
- 前端 Chat、Activity、Workspace、FileTree 分离。
- 每个组件尽量保持单一职责；超过约 300 行时优先考虑继续拆分。

## 9. Swagger

后端统一提供 OpenAPI/Swagger UI：

```text
http://localhost:8080/docs
```

OpenAPI JSON：

```text
http://localhost:8080/api-doc/openapi.json
```

所有 HTTP API 都应在 OpenAPI 中登记；WebSocket endpoint 也登记其用途和路径。

## 10. 当前演进顺序

1. Codex / Pi 原生 Session 恢复
2. Codex / Pi 原生事件结构化
3. 前端 Agent Activity 终端化
4. 文件修改后 Workspace / Diff 自动刷新
5. 独立 OpenCode Adapter
6. 独立 Claude Code Adapter
7. 独立 OpenClaw Adapter
8. 各 Agent 独立模型发现与配置适配

核心原则：**Agent Web 负责统一体验，但不强迫不同 Agent 使用相同的内部实现。**
