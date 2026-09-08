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
- **持久化**：SQLite、用户配置、工作区和 Runtime 均统一保存到 `data/`
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

Docker 模式只需要挂载一个目录：

```yaml
volumes:
  - ./data:/data
```

`data/` 内部包含数据库、Agent 配置、Workspace、Node.js Runtime 和按需安装的 Agent。

## 本地开发

如果需要修改 Rust 后端或 React 前端，可以直接在本地运行，不需要每次重新构建 Docker 镜像。

### 环境要求

- Rust stable / Cargo
- Node.js 22+
- npm

本地开发默认将持久化数据统一放在项目根目录的 `data/` 下，不要求创建 `/data`、`/workspaces`、`/opt/agent-runtimes` 等系统目录。

### 1. 启动后端

```bash
cd backend
cargo run
```

后端默认监听：

```text
http://localhost:8080
```

首次启动时，如果 SQLite 中还没有用户，会自动创建 `admin` 用户，并在终端输出一次随机生成的密码，请保存该密码。

默认数据库：

```text
./data/agentweb.db
```

如果需要指定其他位置，可以通过 `DATABASE_URL` 覆盖。

### 2. 构建前端

```bash
cd frontend
npm install
npm run build
```

### 3. 格式化代码

格式化 Rust：

```bash
make fmt
```

只格式化前端：

```bash
make fmt-frontend
```

前端使用 Prettier 进行格式化。

## 数据目录

推荐项目目录保持：

```text
agentweb/
├── data/
│   ├── agentweb.db
│   ├── home/                 # Agent 的 HOME、配置、缓存和数据
│   ├── workspaces/           # Session 工作区
│   └── runtimes/             # Node.js Runtime 和用户安装的 Agent
├── backend/
├── frontend/
└── docker/
```

Docker 中对应：

| 容器目录 | 用途 |
|---|---|
| `/data/agentweb.db` | SQLite 数据库 |
| `/data/home` | Agent HOME、配置、缓存和数据 |
| `/data/workspaces` | Agent 工作区 |
| `/data/runtimes/node` | Node.js Runtime |
| `/data/runtimes/agents` | 用户安装的 Agent 运行时数据 |

因此重新创建容器后，只要保留宿主机 `./data`，数据库、Agent 配置、Workspace 和 Runtime 都可以继续使用。

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

### 2. 创建会话

创建 Session 时选择 Agent、Workspace 和模型，然后在浏览器中发送任务。

### 3. 查看实时结果

Agent Web 使用 WebSocket 接收 Agent 的实时事件，可以展示文本输出、Thinking、Tool 调用及输出、Command 输出、文件变化、错误和完成状态。

### 4. 查看 Workspace

Agent 在指定 Workspace 中执行任务，Web 界面可以查看文件树、文件内容和 Git Diff。

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

Agent Web 负责 Web UI、Session、消息记录、WebSocket、Agent 管理、Runtime 管理和 Workspace 展示；Agent 负责推理、模型调用、工具执行、Shell、文件修改、MCP、Skills 和子 Agent。

## API

完整 API 文档启动后访问：

```text
http://localhost:8080/docs
```

OpenAPI JSON：

```text
http://localhost:8080/api-doc/openapi.json
```

完整架构和接口设计见 [`TECHNICAL_DESIGN.md`](TECHNICAL_DESIGN.md)。

## 文档

- [技术方案](TECHNICAL_DESIGN.md)
- [English README](README_en.md)

## 项目状态

项目目前处于持续开发阶段，Agent Runtime 安装、Session 恢复和更多 Agent Adapter 仍在持续完善。

## License

License 以仓库当前声明为准。
