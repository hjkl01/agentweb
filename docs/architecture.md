# Agent Web 技术方案

## 1. 总体架构

Agent Web 是 Agent Runtime 的 Web 交互层，不替代 Codex、Pi、OpenCode、Claude Code、OpenClaw 的推理和工具执行。

```text
Browser
  │ HTTPS / HTTP + WebSocket
  ▼
Nginx / Caddy（可选反向代理）
  │
  ▼
Agent Web Backend :8080
  ├── Authentication / User Management
  ├── Agent Registry / Model Discovery
  ├── Session / SQLite
  ├── Workspace / Diff
  └── Agent Manager
       ├── Codex Adapter
       ├── Pi Adapter
       ├── OpenCode Adapter
       ├── OpenClaw Adapter
       └── Generic Adapter
```

## 2. Agent 与模型

每个 Agent 独立适配启动参数、native session/thread、模型发现和事件格式。当前优先 Codex/Pi；其他 Agent 不假设使用相同配置文件。

- Codex：`CODEX_HOME/config.toml` 或 `~/.codex/config.toml`。
- Pi：Agent 自己的 model registry / `pi --list-models`，并兼容其配置路径。
- OpenCode、OpenClaw、Claude Code：后续分别实现原生配置和事件适配。

模型属于 Web Session 配置，可以在新建对话时选择，也可以使用 `PUT /api/sessions/{id}/model` 切换。

## 3. Session / Workspace

Web Session ID 与 Agent native Session/Thread ID 分离。Session 保存 agent、model、workspace 和 native ID；Agent Adapter 负责恢复原生会话。

Workspace 使用 session 独立目录，所有路径请求必须限制在 workspace root 内。Diff 和文件预览有大小限制，并忽略 `.git`、`node_modules`、构建目录等。

## 4. Streaming

Backend 通过 WebSocket 广播 session-scoped events。message/thinking/tool/command/file/error 等事件统一到 AgentEvent；完成的 assistant message 持久化 SQLite。前端重新打开页面时先加载历史消息，再连接 WebSocket。

## 5. 认证与安全

### 5.1 Session Cookie

登录成功后生成随机 opaque token。浏览器只保存 `HttpOnly` Cookie，数据库只保存 token SHA-256 Hash；默认有效期 30 天。API 仍兼容 HTTP Basic Authentication，方便 Swagger/脚本调用。

### 5.2 登录失败锁定

登录失败按客户端 IP 统计：连续错误达到 **3 次**后，该 IP **锁定 30 分钟**。成功登录后清除该 IP 的失败计数。

反向代理必须正确传递 `X-Forwarded-For` 或 `X-Real-IP`；生产环境应只信任来自可信代理的这些 Header，避免客户端伪造 IP。

### 5.3 用户管理

当前提供：

- 当前用户信息
- 修改密码
- 查看所有有效登录 Session
- 标记当前设备
- 一键退出其他设备
- 退出当前账号

修改密码后保留当前 Session，并自动撤销该用户其他 Session。

API：

```text
GET  /api/auth/me
POST /api/auth/login
POST /api/auth/logout
PUT  /api/auth/password
GET  /api/auth/sessions
POST /api/auth/sessions/revoke-all
```

首次创建数据库时生成 12 位随机管理员密码，保证包含大写、小写、数字和特殊字符，只在启动日志显示一次。

## 6. 前端

前端为 SPA，登录页独立于工作区。工作区按 hooks/components/lib/styles 拆分；Session 页面支持响应式布局、自动滚动和 Activity 聚合。账号入口包含密码修改和 Session 管理。

## 7. Swagger

Swagger UI：`http://localhost:8080/docs`；OpenAPI：`/api-doc/openapi.json`。所有 HTTP API 应登记到 OpenAPI。

## 8. 部署

Agent Web 默认监听 `0.0.0.0:8080`。生产环境推荐由 Nginx 或 Caddy 终止 TLS，再反向代理到 8080，并透传 WebSocket Upgrade。

示例：`docs/nginx.conf`、`docs/Caddyfile`。

### Nginx

```text
Client HTTPS → Nginx :443 → Agent Web :8080
```

### Caddy

```text
Client HTTPS → Caddy :443 → Agent Web :8080
```

Caddy 可直接申请 Let's Encrypt 证书；Nginx 通常需要单独配置证书。

## 9. Docker / 开发

本地开发使用 `make install` 安装依赖、`make dev` 启动 frontend/backend。Docker 持久化 `/data`、`/workspaces`、`/opt/agent-runtimes`，Agent 按需安装，不把所有 Agent 强制打进镜像。

## 10. 数据库

SQLite 使用 `schema_meta` 做幂等迁移。认证 Session 位于 `auth_sessions`，与 users 关联。删除数据库后会重新初始化 schema、内置 Agent 和默认 admin。

## 11. 代码组织原则

文件保持单一职责；超过约 300 行优先拆分。Agent-specific 行为进入 agents 子模块，HTTP handler 按 agents/runtime/sessions/workspace/auth 分离。GitHub 修改大文件前先读取当前内容和 SHA，避免覆盖无关改动。
