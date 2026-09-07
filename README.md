# Agent Web

> 一个轻量、Docker-first 的 Web Agent 工作台。

Agent Web 是一个面向 Codex、Claude Code、OpenCode、Pi、OpenClaw 等 Agent 的统一 Web 交互层。它负责 Web UI、会话、实时通信、工作区和 Agent 管理；真正的推理、模型调用、工具执行和代码修改由用户选择的 Agent 完成。

**Agent Web 不是另一个 Agent Runtime。**

## 特性

- **Rust 后端**：Axum + Tokio
- **React 前端**：浏览器中直接使用 Agent
- **SQLite**：不需要 PostgreSQL、Redis 等外部服务
- **Docker 单容器**：Web 与 Agent 在同一个容器中运行
- **Agent 按需安装**：Agent 不预装，由用户从 Web 界面安装
- **Node.js 多版本 Runtime**：Node.js 不作为 Agent Web 后端依赖，可按需安装
- **WebSocket 实时输出**：实时显示 Agent 执行事件
- **Workspace**：文件树、文件预览、Git Diff
- **持久化**：SQLite、用户配置、工作区和 Runtime 均可挂载保存
- **国内镜像优先**：Docker 构建依赖使用国内镜像

## 快速开始

### 1. 克隆

```bash
git clone https://github.com/hjkl01/agentweb.git
cd agentweb
```

### 2. 启动

```bash
docker compose up -d --build
```

然后访问：

```text
http://localhost:8080
```

## Docker 持久化

默认 `docker-compose.yml`：

```yaml
volumes:
  - ./data:/data
  - ./workspaces:/workspaces
  - ./runtimes:/opt/agent-runtimes
```

| 容器目录 | 用途 |
|---|---|
| `/data` | SQLite、用户配置和 Agent 配置 |
| `/workspaces` | Agent 工作区 |
| `/opt/agent-runtimes` | Node.js Runtime 和用户安装的 Agent |

因此重新创建容器后，可以保留数据库、工作区和已经安装的 Runtime。

## 使用方法

### 1. 安装 Agent

启动后进入 Agent 管理界面，选择需要使用的 Agent。

目前 Web Catalog 包含：

```text
Codex
Claude Code
OpenCode
Pi
OpenClaw
```

需要 Node.js 的 Agent，在安装前先选择并安装对应的 Node.js Runtime。

### 2. 选择 Node.js Runtime

Node.js 按版本独立安装，例如：

```text
/opt/agent-runtimes/node/
├── v20.x.x/
├── v22.x.x/
└── ...
```

设计目标是允许不同 Agent 使用不同 Node.js 版本，例如：

```text
OpenCode -> Node.js 22.x
Pi       -> Node.js 20.x
```

### 3. 创建会话

创建 Session 时选择：

- Agent
- Workspace
- 会话名称

然后在浏览器中发送任务。

### 4. 查看实时结果

Agent Web 使用 WebSocket 接收 Agent 的实时事件，可以展示：

- 文本输出
- Thinking
- Tool 调用及输出
- Command 输出
- 文件变化
- 错误
- 完成状态

### 5. 查看 Workspace

Agent 在指定 Workspace 中执行任务，Web 界面可以查看：

- 文件树
- 文件内容
- Git Diff
- 生成的文件

## Agent Web 与 Agent 的关系

```text
浏览器
   │
   ▼
Agent Web
Rust + React + SQLite
   │
   ▼
用户选择的 Agent
   │
   ├── Codex
   ├── Claude Code
   ├── OpenCode
   ├── Pi
   └── OpenClaw
   │
   ▼
模型 / Tools / MCP / Skills / Shell / 文件操作
```

Agent Web 负责：

- Web UI
- Session
- 消息记录
- WebSocket
- Agent 管理
- Runtime 管理
- Workspace 展示

Agent 负责：

- 推理
- 模型调用
- 工具执行
- Shell
- 文件修改
- MCP
- Skills
- 子 Agent

## API

当前主要接口：

```text
GET  /api/health
GET  /api/agents
POST /api/agents
GET  /api/agent-catalog
GET  /api/node/versions
POST /api/node/install
GET  /api/agents/{id}/status
POST /api/agents/{id}/install

GET    /api/sessions
POST   /api/sessions
GET    /api/sessions/{id}
DELETE /api/sessions/{id}
GET    /api/sessions/{id}/messages
POST   /api/sessions/{id}/messages
POST   /api/sessions/{id}/interrupt

GET /api/sessions/{id}/files
GET /api/sessions/{id}/file/{path}
GET /api/sessions/{id}/diff
WS  /api/sessions/{id}/events
```

完整架构和接口设计见 [`TECHNICAL_DESIGN.md`](TECHNICAL_DESIGN.md)。

## 文档

- [技术方案](TECHNICAL_DESIGN.md)
- [English README](README_en.md)

## 项目状态

项目目前处于持续开发阶段，Agent Runtime 安装、不同 Agent 的独立 Runtime 绑定、Session 恢复和更多 Agent Adapter 仍在持续完善。

技术方案中的“目标设计”和“当前实现”会明确区分，避免文档与代码状态混淆。

## License

License 以仓库当前声明为准。
