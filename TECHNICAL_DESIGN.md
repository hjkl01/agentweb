# Agent Web 技术方案

> 本文描述 Agent Web 的架构、模块边界、数据模型和运行时设计。
>
> **重要：本文同时区分“当前实现”和“目标设计”。** 当前实现以代码为准，目标设计用于指导后续开发。

## 1. 项目定位

Agent Web 是一个 Agent Web Console / Unified Agent Workspace，而不是新的 Agent Runtime。

核心职责：

- Web UI
- Session 管理
- 消息持久化
- WebSocket 实时通信
- Agent Catalog
- Agent 安装与配置
- Node.js Runtime 管理
- Workspace / Diff 展示

Agent 自身负责：

- 推理
- 模型调用
- Tool 执行
- Shell
- 文件修改
- MCP
- Skills
- 子 Agent

因此 Agent Web 不重复实现 Agent Loop、LLM Provider Runtime 或通用 Tool Runtime。

## 2. 总体架构

```text
Browser
   |
   | HTTP / WebSocket
   v
+--------------------------------+
|          Agent Web             |
|                                |
| React + TypeScript              |
| Rust + Axum + Tokio             |
| Session Manager                 |
| Agent Adapter                   |
| Installation Manager            |
| Workspace                       |
| SQLite                          |
+---------------+----------------+
                |
                v
        User-installed Agents
     +----------+----------+----------+
     |          |          |          |
   Codex    Claude Code  OpenCode     Pi ...
     |          |          |          |
     +----------+----------+----------+
                |
             Model / Tools
```

Agent Web 与 Agent 位于同一个 Docker 容器中，Agent 可以直接访问挂载的 Workspace。

## 3. 技术栈

| 层 | 当前技术 |
|---|---|
| Frontend | React + TypeScript + Vite |
| Backend | Rust |
| Web Framework | Axum |
| Async Runtime | Tokio |
| Serialization | Serde |
| Database | SQLite |
| Database Access | SQLx |
| Realtime | WebSocket |
| Deployment | Docker |

后端不依赖 Python、PostgreSQL、Redis 或其他外部基础设施。

前端当前以 React/Vite 为主，编辑器、终端等高级组件按功能逐步完善。

## 4. Agent 边界

### Agent Web 不做

```text
Agent Loop
LLM Provider SDK / Provider Runtime
Generic Tool Runtime
Shell Tool Runtime
Python Tool Runtime
Browser Tool Runtime
MCP Runtime
Skills Runtime
Memory Runtime
RAG Runtime
Vector Database
Task Queue
Redis
PostgreSQL
```

### Agent Web 做

```text
Agent Catalog
Agent Installation
Agent Configuration
Agent Process Lifecycle
Session Lifecycle
Message Persistence
Event Normalization
WebSocket Delivery
Workspace Display
Diff Display
Runtime Selection
```

如果 Agent 自己提供 MCP、Skills、Shell、浏览器、子 Agent 等能力，Agent Web 只负责把 Agent 的事件和结果呈现给用户。

## 5. Agent Adapter

Rust 使用统一接口隔离不同 Agent 的启动方式和输出协议：

```rust
#[async_trait]
pub trait AgentAdapter: Send + Sync {
    async fn start(&self, config: &AgentConfig) -> Result<()>;

    async fn send_message(
        &self,
        config: &AgentConfig,
        session_id: &str,
        message: &str,
        events: &EventBus,
    ) -> Result<AgentRunResult>;

    async fn interrupt(&self, session_id: &str) -> Result<()>;
}
```

当前代码采用基于进程的 Adapter，并按 Agent 类型处理命令行参数和 JSON 输出。

当前已存在的主要实现方向：

```text
agents/
├── mod.rs
├── adapter.rs
├── process.rs
├── codex.rs
├── opencode.rs
├── pi.rs
└── generic.rs
```

### 目标

未来优先使用 Agent 提供的标准协议，例如 ACP；没有标准协议时保留专用 Adapter。

## 6. Agent 运行模型

一次 Session 的基本流程：

```text
Create Session
      |
      v
Select Agent
      |
      v
Select Workspace
      |
      v
Resolve Agent Installation
      |
      v
Start / Resume Agent
      |
      v
Agent Events
      |
      v
Normalize Events
      |
      v
WebSocket -> Browser
```

Agent 的 native session ID 应保存到 Web Session 中，用于 Agent 自己支持的 Session resume。

当前代码已经为 `sessions.native_session_id` 提供持久化。

## 7. Agent Catalog 与安装

Agent Catalog 是用户安装 Agent 的入口，不应该演变成通用包管理器。

每个受支持 Agent 应由明确的定义控制：

```text
AgentDefinition
  id
  name
  description
  executable
  install_method
  install_command
  version_command
  requirements
  runtime_type
  adapter
```

例如：

```text
Codex
  runtime: native / configured executable

Claude Code
  runtime: node

OpenCode
  runtime: node

Pi
  runtime: node

OpenClaw
  runtime: according to its installation requirements
```

### 安装要求

- 安装必须由后端受控执行
- 安装过程通过事件反馈给 Web UI
- 安装结果必须能够检测
- Agent 版本必须能够检测
- Agent 安装文件必须落在持久化 Runtime 目录

## 8. Node.js Runtime

Node.js 不是 Agent Web 后端依赖。

用户需要哪个 Node.js 版本，由 Web UI 选择后安装。

目标目录：

```text
/opt/agent-runtimes/node/
├── v20.x.x/
├── v22.x.x/
└── ...
```

每个 Node Runtime 应包含独立的：

```text
bin/node
bin/npm
bin/npx
```

### 版本展示规则

Node.js release metadata 应动态获取，而不是长期硬编码版本。

UI 按 Major 分组，并对每个 Major 展示最新 3 个版本：

```text
Node 20
  20.x.x
  20.x.x
  20.x.x

Node 22
  22.x.x
  22.x.x
  22.x.x
```

这样可以避免每次 Node 发布新版本都修改代码。

### 下载源

运行时下载采用：

```text
1. 国内 Node 镜像
       |
       | 失败
       v
2. nodejs.org 官方源
```

Docker 构建阶段同样优先使用国内镜像。

## 9. Agent 与 Node Runtime 绑定

这是 Runtime 设计的关键部分。

不同 Agent 不应该共享一个全局“当前 Node”。例如：

```text
OpenCode -> Node 22.20.0
Pi       -> Node 20.19.4
```

目标目录：

```text
/opt/agent-runtimes/
├── node/
│   ├── v20.19.4/
│   └── v22.20.0/
│
└── agents/
    ├── opencode/
    │   └── node-22.20.0/
    └── pi/
        └── node-20.19.4/
```

启动 Agent 时：

```text
Agent PATH
    = Agent bin
    + selected Node bin
    + system PATH
```

同时设置 Agent 自己的安装 Prefix，避免不同 Agent 的 npm global package 互相覆盖。

### 当前实现状态

当前代码已经具备：

- Node Runtime 独立目录
- 用户选择 Node 版本
- Agent 启动时注入 Runtime PATH
- `/opt/agent-runtimes` 持久化

但 Agent 安装记录目前仍主要依赖运行时检测，尚未完全形成 `agent_id + node_version` 的独立数据库模型。

**下一步建议增加：**

```sql
agent_installations
-------------------
agent_id
node_version
version
install_root
installed
created_at
updated_at
PRIMARY KEY(agent_id, node_version)
```

这样可以明确表达：同一个 Agent 是否安装在某个 Node Runtime 下。

## 10. Session

SQLite 中保存 Web 层 Session：

```text
sessions
---------
id
agent_id
title
workspace
status
native_session_id
created_at
updated_at
```

Session 不应该把 Agent 内部状态全部复制到 Agent Web 数据库。

Agent Web 保存的是：

- Web Session ID
- Agent ID
- Workspace
- native session ID
- UI 状态

真正的 Agent 上下文由 Agent 自己管理。

## 11. Message

当前消息模型：

```text
messages
--------
id
session_id
role
content
created_at
```

当前主要保存 user / assistant 文本。

目标是逐步支持结构化消息部分：

```text
Text
Thinking
ToolCall
ToolResult
Command
FileChange
Artifact
Error
```

## 12. 统一事件模型

Agent 原始事件由 Adapter 转换为 Agent Web 的统一事件。

```text
session.started
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
session.completed
install.output
install.completed
```

事件的目标是让前端不需要了解 Agent-specific protocol。

例如：

```json
{
  "type": "tool.started",
  "session_id": "session-123",
  "tool": "shell",
  "input": {
    "command": "git diff"
  }
}
```

## 13. WebSocket

Realtime API：

```text
WS /api/sessions/{id}/events
```

流程：

```text
Agent
  |
  v
Adapter
  |
  v
EventBus
  |
  v
WebSocket
  |
  v
Browser
```

WebSocket 主要负责实时展示，不应该成为持久化的唯一来源。

重要 Session / Message 状态应写入 SQLite。

## 14. Workspace

Workspace 根目录：

```text
/workspaces
```

Session 指向一个 Workspace。

Web 层提供：

- 文件树
- 文件预览
- Git status / diff
- 文件下载

实际文件修改由 Agent 执行。

后端需要持续保持 Workspace 路径安全，防止通过 `..` 等方式逃逸 `/workspaces`。

## 15. Artifact

Agent 可能生成：

```text
.docx
.pptx
.xlsx
.pdf
.png
.zip
```

Agent Web 不负责生成这些文件。

它负责发现、展示、预览或下载这些 Artifact。

## 16. SQLite 数据模型

当前主要表：

```text
agents
sessions
messages
runtime_settings
```

### agents

```text
id
name
kind
command
working_directory
enabled
installed
version
created_at
updated_at
```

### sessions

```text
id
agent_id
title
workspace
status
native_session_id
created_at
updated_at
```

### messages

```text
id
session_id
role
content
created_at
```

### runtime_settings

用于保存 Runtime 层面的选择，例如当前 Runtime 配置。

### 目标新增

```text
agent_installations
```

用于表达 Agent 与 Node Runtime 的明确绑定关系。

## 17. REST API

当前主要 API：

### Health

```http
GET /api/health
```

### Agent

```http
GET  /api/agents
POST /api/agents
GET  /api/agent-catalog
GET  /api/agents/{id}/status
POST /api/agents/{id}/install
```

### Node.js

```http
GET  /api/node/versions
POST /api/node/install
```

### Session

```http
GET    /api/sessions
POST   /api/sessions
GET    /api/sessions/{id}
DELETE /api/sessions/{id}
POST   /api/sessions/{id}/interrupt
```

### Message

```http
GET  /api/sessions/{id}/messages
POST /api/sessions/{id}/messages
```

### Workspace

```http
GET /api/sessions/{id}/files
GET /api/sessions/{id}/file/{path}
GET /api/sessions/{id}/diff
```

### WebSocket

```text
WS /api/sessions/{id}/events
```

## 18. Docker

生产运行时为单容器：

```text
+------------------------------------------------+
| agentweb container                             |
|                                                |
| Rust Axum Server                               |
| React static files                             |
|                                                |
| /data                                          |
| /workspaces                                    |
| /opt/agent-runtimes                            |
|                                                |
| user-installed Agents                          |
+------------------------------------------------+
```

默认 compose：

```yaml
volumes:
  - ./data:/data
  - ./workspaces:/workspaces
  - ./runtimes:/opt/agent-runtimes
```

### 持久化含义

```text
/data
  └── SQLite + HOME/config/cache

/workspaces
  └── 用户项目

/opt/agent-runtimes
  ├── node
  └── agents
```

不需要 Nginx 或 Caddy 才能运行 Agent Web。Axum 直接提供 HTTP API、WebSocket 和 React 静态文件。

如果以后部署到公网，可以在容器外增加 Caddy/Nginx 负责 HTTPS 和反向代理，但它们不是 Agent Web 的运行依赖。

## 19. Docker 下载源

当前 Dockerfile 已尽量使用国内源：

```text
Frontend npm
  -> registry.npmmirror.com

Cargo
  -> rsproxy.cn

Debian apt
  -> mirrors.aliyun.com
```

运行时 Node.js 下载还需要进一步统一为：

```text
npmmirror Node mirror
        |
        | unavailable
        v
nodejs.org
```

同时 Agent 的 npm 安装也应优先使用国内 npm registry，并保留官方 registry fallback 的能力。

## 20. 当前实现状态

### 已实现

- [x] Rust + Axum backend
- [x] React frontend
- [x] SQLite
- [x] Session
- [x] Message persistence
- [x] WebSocket event stream
- [x] Codex / OpenCode / Pi 等 Adapter 基础
- [x] Workspace 文件展示
- [x] Git Diff
- [x] Docker 单容器
- [x] `/data` 持久化
- [x] `/workspaces` 持久化
- [x] `/opt/agent-runtimes` 持久化
- [x] 用户选择 Node.js Runtime
- [x] Node Runtime PATH 注入
- [x] Node 版本按 Major 分组并展示最新版本

### 部分实现

- [ ] Agent 安装模型完全统一
- [ ] Agent 与 Node Runtime 的数据库绑定
- [ ] Session 长生命周期 Agent 进程
- [ ] 更完整的结构化事件
- [ ] ACP Adapter
- [ ] 完整 Agent 版本管理
- [ ] 安装失败回滚
- [ ] WebSocket reconnect / event replay

### 尚未完成

- [ ] Node.js release metadata 动态获取
- [ ] Node.js 国内镜像优先 + 官方 fallback
- [ ] Codex 等所有 Agent 都通过统一 Web Installer 安装
- [ ] Claude Code / OpenCode / Pi / OpenClaw 完整独立 Runtime 管理
- [ ] 多 Agent 安装实例的完整生命周期管理
- [ ] Authentication
- [ ] Permission / sandbox controls

## 21. 安全设计

Agent 本质上可以执行高权限操作，因此 Agent Web 不能把 Agent 当作普通 Web 请求处理器。

至少需要考虑：

1. Workspace 路径限制
2. Agent 安装命令白名单
3. 不允许用户任意提交 shell 安装命令
4. Agent 安装目录与系统目录隔离
5. Environment Variables 的权限边界
6. WebSocket Session 隔离
7. 未来增加 Authentication
8. 未来增加 Agent permission / sandbox

尤其不能因为提供“自定义 Agent”功能，就直接把任意字符串交给 shell 执行。

## 22. 为什么单容器

目标场景是个人、自托管和小规模部署。

因此第一阶段不需要：

```text
Redis
PostgreSQL
Kafka
RabbitMQ
Celery
Kubernetes
```

单容器可以直接共享：

```text
Agent Web
   |
   +-- /workspaces
   |
   +-- /data
   |
   +-- /opt/agent-runtimes
```

未来如果需要多用户、多机器、多 Agent Worker，再考虑拆分服务。

## 23. Roadmap

### V0.1

```text
Rust Server
React UI
SQLite
Docker
Session
WebSocket
Workspace
基础 Agent Adapter
```

### V0.2

```text
统一 Agent Installer
动态 Node.js Release
Node 国内镜像 fallback
Agent / Node Runtime 独立绑定
ACP
Claude Code
OpenCode
Pi
```

### V0.3

```text
OpenClaw
更多 Agent Adapter
Artifact Preview
多 Workspace
Session Replay
```

### V1.0

```text
Unified Agent Web Console
Authentication
Permission Control
Remote Agent
稳定的 Runtime 生命周期管理
```

## 24. 技术决策总结

最终目标保持简单：

```text
                       Agent Web
                           |
             +-------------+-------------+
             |                           |
          React UI                    Rust API
             |                           |
             +-------------+-------------+
                           |
                     SQLite / WS
                           |
                    Agent Adapter
                           |
          +----------------+----------------+
          |                |                |
        Codex          OpenCode            Pi ...
          |                |                |
          +----------------+----------------+
                           |
                    User-selected Model
```

Agent Web 的核心价值不是替代 Codex、Claude Code、OpenCode、Pi 等 Agent，而是提供一个统一、持久、可部署、可扩展的 Web 工作台，让用户可以在同一个界面管理和使用不同 Agent。
