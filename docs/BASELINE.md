# Agent Web 当前基线检查

检查日期：2026-11-03  
检查分支：main  
检查方式：GitHub 仓库静态代码审查。当前执行环境无法解析 github.com，未能克隆仓库，因此本报告不声称本地构建或测试已经通过。

## 1. 构建与 CI

- 后端使用 Rust/Cargo，清单为 `backend/Cargo.toml`。
- 前端使用 Vite，`frontend/package.json` 提供 `build` 脚本；没有引入 Vitest/Jest 等前端测试框架。
- 根目录 Makefile 提供 `make check`、`make test`、`make fmt`、`make lint`。
- 原 CI 只有 `workflow_dispatch`，push 和 pull request 不会自动触发。
- 原 CI 没有执行 `cargo fmt --check`，也没有运行 Makefile 中的前端纯逻辑检查脚本。
- 已调整 CI：对 main 的 push 和 pull request 自动执行前端构建、名称检查、前端纯逻辑检查、Rust 格式检查、cargo check、cargo test 和 Docker 构建；保留手动触发入口。

## 2. 测试现状

仓库没有独立的 `backend/tests/` 集成测试目录；Rust 测试主要以内嵌 `#[cfg(test)]` 模块的形式分布在源码中。通过仓库代码搜索确认，测试覆盖涉及：

- Agent Adapter 配置、AgentManager 路由、命令构造、模型、MCP、Plugins。
- Agent 进程并发启动、停止、子进程组清理、原生 Session 丢失识别。
- Pi RPC、Pi Transcript 和事件解析。
- Session 生命周期、Workspace、Filesystem、Worktree。
- Scheduler 的 Cron/Interval 计算、租约、任务领取和恢复。
- EventBus 的事件序列化、Relay、事件持久化边界。
- 认证和运行时安装。

前端测试由 `frontend/scripts/check-frontend.mjs` 实现，Makefile 先用 TypeScript 编译指定的纯逻辑模块，再运行 Node.js 断言。它覆盖 Session 分组、偏好解析、格式化、Diff、工具结果关联和待发送消息处理等逻辑，但不是 React 组件或浏览器端到端测试。

限制：
- 静态存在测试不代表测试实际通过；本次未能运行 `cargo test`、`npm run build` 或 Docker 构建。
- 尚未发现覆盖六种真实 Agent CLI 的完整集成测试矩阵。Mock/进程层测试不能替代真实 CLI 的登录、启动、继续会话、停止和异常恢复验证。
- CI 改动提交后，应以 GitHub Actions 的实际结果作为第一轮动态验证。

## 3. 已确认的可靠性缺口

### 3.1 Cron 时区（已实现，待 CI 验证）

已引入 `chrono-tz`，`backend/src/scheduler.rs` 的 `next_cron()` 现在按 IANA 时区解析，并在 UTC 时间轴逐分钟扫描、转换成本地时间匹配 Cron。创建/更新 API 已移除原先重复的 UTC-only Cron 计算，改为复用调度器函数；前端 Cron 表单增加 IANA 时区输入框。新增上海时区、纽约夏令时开始/结束和非法时区测试。

DST 策略：春季跳时导致的不存在本地时间会跳过；秋季回拨造成的重复本地时间会分别触发两次。当前五段 Cron 表达式仍使用项目原有字段匹配语义，不在本次范围内扩展为完整 Quartz/Cron 特性。

### 3.2 实时事件与重连

`backend/src/events.rs` 使用 Tokio broadcast 和有界 mpsc 队列。跨 Worker Relay 明确是 best-effort：队列满、关闭或连接异常时会丢弃实时事件。Session 对话事件不写入 SQLite，只有安装状态等事件会持久化。

前端 `useSessionEvents` 已实现心跳、断线指数退避重连、先订阅再加载 HTTP 快照，并缓冲快照加载期间到达的事件。此前后端收到 broadcast `Lagged` 时只跳过事件继续推送，可能让前端长期保留不完整的当前轮次；现已改为发送 `session.resync_required` 并关闭连接，借助既有重连流程重新拉取状态快照。跨 Worker Relay 队列满时仍然只是记录日志并丢弃事件，尚未实现端到端的 resync 通知。

### 3.3 Agent 生命周期

`backend/src/agents/process.rs` 已有进程组管理、kill-on-drop、并发执行保护、interrupt 和原生 Session 丢失识别等实现与测试。仍需针对 Codex、Claude、Gemini、OpenCode、Pi、Qwen 六种真实 CLI 建立端到端兼容性矩阵，特别是启动、resume、停止、进程异常退出和服务重启。

### 3.4 原生 Transcript 恢复

`backend/src/api/session_messages.rs` 已支持 Pi 和 Codex 原生 Transcript。Codex 读取器从 `CODEX_HOME/sessions`（默认 `~/.codex/sessions`）递归查找匹配 thread ID 的 rollout JSONL，只提取 `response_item` 中的 message、function_call 和 function_call_output，不读取重复的 `event_msg`。读取在受限的 blocking worker 中执行，不将聊天正文写入 SQLite。当前限制：尚未支持 Codex 压缩归档 `.jsonl.zst`；目录扫描有深度和条目上限，找不到文件或读取失败时暂时返回空消息列表，尚未像 Pi 一样区分 Missing/Unknown。Claude、Gemini、OpenCode、Qwen 仍返回空数组。

## 4. 本轮后续顺序

1. 等待自动 CI 对 CI 配置和 Cron 时区改动执行并检查结果。
2. 处理 WebSocket 重连后的状态快照和 Transcript 恢复。
3. 补齐六种 Agent 的生命周期验证矩阵。

## 5. 未验证事项

以下事项不能仅凭静态阅读判定通过：

- Rust 格式、编译、单元测试和 Clippy。
- 前端生产构建和纯逻辑检查。
- Docker 镜像构建与容器启动。
- 六种 Agent 在实际用户环境中的 CLI 兼容性。
