# Agent Web

> 一个轻量、Docker 优先的 AI Agent Web 控制台。

Agent Web 面向 Codex、Claude Code、OpenCode、Pi、OpenClaw 等现有 Agent，提供统一的 Web 交互、会话、工作区和运行管理能力。

**Agent Web 不是另一个 Agent Runtime。** Agent 本身的推理、模型调用、工具执行、Shell、文件修改、MCP、Skills、子 Agent 等能力，仍由用户选择的 Agent 负责。

> English documentation: [README_en.md](README_en.md)

## 1. 项目定位

Agent Web 是 **Agent Web Console / Unified Agent Workspace**，而不是 Agent Runtime。

```text
浏览器
   |
   | HTTP / WebSocket
   v
+-----------------------------+
|          Agent Web          |
|                             |
| React 前端                  |
| Rust 后端                   |
| Agent Adapter               |
| SQLite                      |
+--------------+--------------+
               |
               v
        Agent Runtime
    /      |       |      \
  Codex  Claude   OpenCode  Pi ...
    |      Code       |      |
  Model   Model     Model  Model
```

核心边界：

- **Agent Web**：Web UI、会话、消息、实时事件、工作区、文件展示、Agent 配置与安装管理。
- **Agent**：推理、模型调用、工具、Shell、文件编辑、MCP、Skills、子 Agent 以及实际任务执行。

## 2. 核心原则

### 2.1 Agent First

选中的 Agent 是真正的执行引擎。Agent Web 不重新实现 Agent Loop，也不实现一套通用 Tool Runtime。

### 2.2 Model Agnostic

Agent Web 不直接负责 Agent 的 LLM 调用。使用哪个模型、哪个 Provider，由具体 Agent 自己决定。

例如：

```text
Agent Web -> Codex       -> Model
Agent Web -> Claude Code -> Model
Agent Web -> OpenCode    -> Model
Agent Web -> Pi          -> Model
```

### 2.3 统一 Adapter

不同 Agent 的启动方式、通信协议和事件格式不同。Agent Web 通过 `AgentAdapter` 将它们统一成 Web 层可以理解的事件流。

如果 Agent 支持 ACP，优先考虑通过 ACP 集成；不支持时使用对应 Agent 的专用 Adapter。

### 2.4 轻量部署

默认使用一个 Docker 容器，同时运行 Rust Web 服务和用户安装的 Agent。

不依赖 Redis、PostgreSQL、Kafka、RabbitMQ、Celery 等外部基础设施。

## 3. 技术栈

| 层 | 技术 |
|---|---|
| 前端 | React + TypeScript + Vite |
| UI | Tailwind CSS |
| 状态管理 | Zustand |
| API | REST |
| 实时通信 | WebSocket |
| 后端 | Rust |
| Web Framework | Axum |
| 异步运行时 | Tokio |
| 序列化 | Serde |
| 数据库 | SQLite |
| 数据库访问 | SQLx |
| 代码编辑器 | Monaco Editor |
| 终端 UI | xterm.js |
| 部署 | Docker |

后端不使用 Python。

Node.js 也不是 Agent Web 后端的运行时依赖。需要 Node.js 的 Agent，由用户在 Web 界面中选择 Node.js 版本后安装。

## 4. Docker 架构

Agent Web、Agent 和 Web 前端位于同一个容器中。

推荐持久化：

```yaml
volumes:
  - ./data:/data
  - ./workspaces:/workspaces
  - ./runtimes:/opt/agent-runtimes
```

其中：

- `/data`：SQLite、用户配置、Agent 配置和缓存。
- `/workspaces`：项目工作区。
- `/opt/agent-runtimes`：Node.js Runtime 和用户安装的 Agent。

## 5. Agent 安装模型

**Docker 镜像默认不预装 Codex、Claude Code、OpenCode、Pi、OpenClaw 等 Agent。**

Agent Web 只提供 Agent Catalog，用户从 Web 界面选择并安装：

```text
Agent Catalog

Codex
[Install]

Claude Code
Requires: Node.js
[Install]

OpenCode
Requires: Node.js
[Install]

Pi
Requires: Node.js
[Install]

OpenClaw
[Install]
```

安装流程：

```text
Agent Catalog
      |
      v
Installation Manager
      |
      +---- Node.js Runtime
      |
      +---- Agent Runtime
      |
      v
Agent Adapter
```

安装过程通过 WebSocket 向前端推送进度，而不是长时间阻塞浏览器请求。

Agent 安装不是通用包管理器。每个支持的 Agent 都由 Agent Web 内置的安装定义控制：

```text
AgentDefinition
  name
  id
  description
  executable
  install_method
  install_command
  version_command
  requirements
  adapter
```

## 6. Node.js Runtime

Node.js 不放入 Agent Web 默认运行镜像。

用户可以在 Web 界面中选择需要的 Node.js 版本。版本按照 Major 版本分组，每个 Major 默认展示最新的 3 个版本。

不同 Agent 可以使用不同的 Node.js Runtime：

```text
Node.js 22.20.0
    |
    +-- OpenCode

Node.js 20.19.4
    |
    +-- Pi
```

目录结构类似：

```text
/opt/agent-runtimes/
├── node/
│   ├── v22.20.0/
│   └── v20.19.4/
│
└── agents/
    ├── opencode/
    │   └── node-22.20.0/
    └── pi/
        └── node-20.19.4/
```

这样可以避免不同 Agent 之间的 Node.js 和 npm 全局包互相污染。

## 7. 国内镜像

Docker 构建和运行时下载尽量优先使用国内镜像。

当前 Docker 构建阶段包括：

```text
npm      -> registry.npmmirror.com
Cargo    -> rsproxy.cn
Debian   -> mirrors.aliyun.com
```

Node.js Runtime 安装也应采用国内源优先、官方源作为 fallback 的方式。

## 8. Agent Adapter

Rust 定义统一的 Agent 接口：

```rust
#[async_trait]
pub trait AgentAdapter: Send + Sync {
    async fn start(&self, config: AgentConfig) -> Result<()>;

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

Agent 适配器负责处理具体 Agent 的启动、参数、Session 恢复和事件解析。

预期支持：

```text
agents/
├── mod.rs
├── adapter.rs
├── codex.rs
├── opencode.rs
├── pi.rs
├── generic.rs
└── ...
```

## 9. 统一事件模型

Agent 的原始输出会被转换成统一事件：

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
file.created
file.modified
file.deleted
command.started
command.output
command.completed
agent.error
session.completed
```

前端不需要知道事件来自 Codex、OpenCode、Pi 还是其他 Agent。

## 10. WebSocket

实时 Agent 交互使用：

```text
WS /api/sessions/{session_id}/events
```

前端可以实时显示：

- 文本输出
- Thinking
- Tool 调用
- Tool 输出
- Shell/Command 输出
- 文件修改
- 错误
- Agent 完成状态

## 11. Session

Session 由 Agent Web 管理，但真正的 Agent 会话状态由对应 Agent 负责。

```text
创建 Session
    |
    v
选择 Agent
    |
    v
选择 Workspace
    |
    v
启动 Agent
    |
    v
Running
    |
    +----> Waiting
    |
    v
Completed
```

浏览器刷新后，Web 层应尽量恢复已有 Session，而不是强制创建新的 Agent Session。

## 12. Workspace

Agent Web 不实现另一套文件操作 Tool Runtime。实际文件操作仍由 Agent 完成。

Agent Web 主要负责展示：

- 文件树
- 文件预览
- 代码编辑器
- Git Diff
- 文件下载
- Artifact

## 13. Artifact

Agent 可以生成：

```text
.docx
.pptx
.xlsx
.pdf
.png
.zip
```

Agent Web 不负责生成这些文件，而是负责发现和展示它们。

## 14. SQLite

Agent Web 使用 SQLite，适合单容器、自托管部署。

数据库主要保存 Web 交互层数据：

```text
agents
agent configurations
sessions
messages
settings
runtime settings
agent installations
```

数据库持久化到：

```text
/data/agentweb.db
```

项目没有 PostgreSQL 依赖。

## 15. REST API

### Agent

```http
GET  /api/agents
POST /api/agents
GET  /api/agent-catalog
GET  /api/agents/{id}/status
POST /api/agents/{id}/install
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

### Node.js

```http
GET  /api/node/versions
POST /api/node/install
POST /api/node/activate
```

### Realtime

```text
WS /api/sessions/{id}/events
```

## 16. 前端界面

整体采用类似 AionUi 的工作区模式：

```text
┌─────────────────────────────────────────────────────────┐
│ Agent Web                                               │
├──────────────┬──────────────────────────┬───────────────┤
│ Sessions     │ Conversation             │ Workspace     │
│              │                          │               │
│ + New Chat   │ User message             │ Files         │
│              │                          │               │
│ Codex        │ Agent output             │ src/          │
│ Claude Code  │                          │ package.json  │
│ OpenCode     │ Tool calls               │ README.md     │
│ Pi           │ File changes             │               │
│              │                          │ Diff          │
├──────────────┴──────────────────────────┴───────────────┤
│ Message input                                  Send    │
└─────────────────────────────────────────────────────────┘
```

Agent 管理界面同时显示：

- 已安装 Agent
- 未安装 Agent
- Agent 所需 Node.js
- Agent 使用的 Node.js 版本
- 安装状态
- 版本信息

## 17. 项目结构

```text
agentweb/
├── frontend/
│   ├── src/
│   │   ├── components/
│   │   ├── pages/
│   │   ├── features/
│   │   │   ├── chat/
│   │   │   ├── sessions/
│   │   │   ├── agents/
│   │   │   ├── workspace/
│   │   │   └── settings/
│   │   ├── stores/
│   │   ├── api/
│   │   └── types/
│   └── package.json
│
├── backend/
│   ├── src/
│   │   ├── main.rs
│   │   ├── api.rs
│   │   ├── agents/
│   │   ├── sessions/
│   │   ├── workspace/
│   │   ├── installation/
│   │   ├── events.rs
│   │   ├── db.rs
│   │   └── state.rs
│   ├── migrations/
│   └── Cargo.toml
│
├── docker/
│   └── Dockerfile
│
├── docker-compose.yml
├── README.md
└── README_en.md
```

## 18. Docker 持久化

推荐：

```yaml
services:
  agentweb:
    build:
      context: .
      dockerfile: docker/Dockerfile
    restart: unless-stopped
    ports:
      - "8080:8080"
    volumes:
      - ./data:/data
      - ./workspaces:/workspaces
      - ./runtimes:/opt/agent-runtimes
```

这样删除并重新创建容器后，以下内容仍然保留：

```text
SQLite
用户配置
Session 数据
Workspace
Node.js Runtime
Agent 安装目录
```

## 19. MVP

### Agent 管理

- Agent Catalog
- 用户手动安装 Agent
- 用户选择 Node.js 版本
- 不同 Agent 使用不同 Node.js Runtime
- Agent 配置
- Agent 状态和版本检测

### Session

- 创建 Session
- 选择 Agent
- 选择 Workspace
- Session 列表
- Session 恢复
- Session 删除

### Chat

- 发送消息
- 流式输出
- 停止 Agent
- WebSocket 重连

### Events

- Text
- Thinking
- Tool Call
- Tool Result
- File Change
- Command
- Error
- Completed

### Workspace

- 文件树
- 文件预览
- 代码编辑器
- Diff
- Artifact
- 下载

### Storage

- SQLite

### Deployment

- Docker
- 单容器
- 持久化 `/data`
- 持久化 `/workspaces`
- 持久化 `/opt/agent-runtimes`

## 20. 明确不做的事情

以下能力不属于 Agent Web Core：

```text
Agent Loop
LLM Provider SDK
Shell Tool
Python Tool
Browser Tool
MCP Runtime
Skills Runtime
Memory Runtime
RAG Runtime
Vector Database
Task Queue
Redis
PostgreSQL
Kafka
RabbitMQ
Celery
```

如果这些能力由 Agent 本身提供，Agent Web 只负责展示对应事件和结果，不重复实现。

## 21. Roadmap

### V0.1

```text
Rust Server
React UI
SQLite
Agent Adapter
WebSocket
Session
Workspace
Docker
```

### V0.2

```text
ACP
Agent Catalog
One-click Agent Installation
Node.js Runtime Manager
Codex
Claude Code
OpenCode
Pi
```

### V0.3

```text
更多 Agent Adapter
Remote Agent
Artifact Preview
Multiple Workspaces
Authentication
Permission Controls
```

### V1.0

```text
Unified Agent Web Console
```

## 22. 设计总结

Agent Web 的核心架构保持简单：

```text
                     Agent Web
                         |
             +-----------+-----------+
             |                       |
          React UI              Rust Backend
                                     |
             +-----------------------+----------------+
             |                 |                       |
           SQLite          Adapter                Installer
                               |                       |
                 +-------------+-------------+         |
                 |             |             |         |
               Codex       OpenCode        Pi      Node.js
                 |             |             |         |
               Model         Model         Model     Runtime
```

最重要的架构原则：

> **Rust 是统一的 Web 网关；用户选择的 Agent 才是真正的大脑和执行引擎。**

这样可以保持 Agent Web 轻量、避免重复实现 Agent 能力、减少基础设施依赖，并让增加新的 Agent 主要变成 Adapter + 安装定义的问题。
