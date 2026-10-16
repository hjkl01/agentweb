# Agent Web

> 一个轻量、Docker-first 的 Web Agent 工作台。

Agent Web 是一个面向 Codex、Claude Code、OpenCode、Pi 等 Agent 的统一 Web 交互层。它负责 Web UI、会话、实时通信、工作区和 Agent 管理；真正的推理、模型调用、工具执行和代码修改由用户选择的 Agent 完成。

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
- **持久化**：SQLite 默认位于 `$HOME/.config/agentweb/agentweb.db`；工作区和 Node.js Runtime 可独立配置
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

Docker 的 `data/` 持久化数据库、Agent HOME/配置和 Docker 专用 Runtime；Workspace 直接映射到宿主机 `$HOME`。宿主机直接运行二进制时，Agent 使用用户自己的 HOME 和已安装/配置的 Agent，不使用 Agent Web Runtime。

## 本地构建并直接启动

在项目根目录执行：

```bash
make install
make build
make start
```

`make build` 会先构建前端，再把前端静态资源嵌入经过优化的 Rust 二进制文件。构建产物只有一个可执行文件，不需要额外携带 `frontend/` 目录：

```text
agentweb/
├── agentweb              # 独立运行的二进制，内含前端
├── backend/
├── frontend/
└── ...
```

构建完成后，运行 `make start` 即可启动服务，默认访问 http://localhost:8080。SQLite 默认保存在 `$HOME/.config/agentweb/agentweb.db`；也可以通过 `DATABASE_URL` 指定其他位置。旧版 `data/agentweb.db` 在首次启动且新数据库不存在时会自动迁移。 `make release` 是 `make build` 的别名。

`make start` 会以前台方式运行，按 `Ctrl+C` 停止。首次启动时，请留意终端输出的默认管理员密码并妥善保存。

## 本地开发

如果需要修改 Rust 后端或 React 前端，可以直接在本地运行，不需要每次重新构建 Docker 镜像。

### 环境要求

- Rust stable / Cargo
- Node.js 22+
- npm
- Linux：宿主机 Agent/终端直接使用当前用户权限运行；Docker 内按容器权限运行

宿主机直接运行二进制时，Agent 使用用户自己的 `HOME`、`~/.config`、`~/.local` 等原生配置目录，并以用户权限直接操作 `$HOME` 下的工作目录。Agent Web 不提供额外的文件系统沙箱。Docker 模式则使用容器内的 `/data/home` 作为 Agent HOME，并将宿主机 `$HOME` 挂载到 `/workspaces`。

Arch Linux 可安装依赖：

```bash
sudo pacman -S bubblewrap
```

本地开发的 SQLite 默认位于 `$HOME/.config/agentweb/agentweb.db`；宿主机 Workspace 默认就是 `$HOME`。宿主机 Agent 不使用 Agent Web Runtime，直接使用用户 PATH 和用户自己的 Agent 配置。

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
$HOME/.config/agentweb/agentweb.db
```

如果需要指定其他位置，可以通过 `DATABASE_URL` 覆盖。首次启动时会自动创建目录；从旧版 `./data/agentweb.db` 升级且新数据库不存在时，会先检查点 WAL，再迁移数据库。

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
├── agentweb                 # make build 生成的独立二进制
├── backend/
├── frontend/
└── docker/

# 宿主机数据
$HOME/.config/agentweb/agentweb.db
$HOME/                           # Session 默认 Workspace
```

Docker 中对应：

| 容器目录 | 用途 |
|---|---|
| `/data/home/.config/agentweb/agentweb.db` | SQLite 数据库 |
| `/data/home` | Docker 中 Agent HOME、配置和缓存 |
| `/workspaces` | 宿主机 `$HOME` 映射后的 Docker Workspace |
| `/data/runtimes/node` | Docker 专用 Node.js Runtime |
| `/data/runtimes/agents` | Docker 中用户安装的 Agent 运行时数据 |

因此重新创建容器后，保留宿主机 `./data` 可以继续使用数据库、Docker 内 Agent 配置和 Runtime；Workspace 则来自宿主机 `$HOME`，不随容器复制。

## 使用方法

### 1. 安装 Agent

启动后进入 Agent 管理界面，选择需要使用的 Agent。

目前 Web Catalog 包含：

```text
Codex
Claude Code
OpenCode
Pi
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
   └── Pi
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
