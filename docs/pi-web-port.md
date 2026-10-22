# Pi Web → Agent Web 功能移植分析

> 对照对象：`agegr/pi-web`（main 分支源码 + 本地已安装 0.12.2）。
> 对照基线：`/home/ljl/github/agentweb` 当前实现。
> 结论用于排期，不代表必须全量移植。

## 0. 定位差异（决定什么该搬、什么不该搬）

|        | Pi Web                                        | Agent Web                                         |
| ------ | --------------------------------------------- | ------------------------------------------------- |
| 目标   | 单个 agent（pi）的本地轻量 Web 壳             | 多 agent 的统一 Web 工作台                        |
| 运行时 | Node/Next.js，进程内 `AgentSession`，SSE      | Rust/Axum + Vite，进程外 Agent Adapter，WebSocket |
| 持久化 | pi 自己的 jsonl + `pi-web-session-state.json` | SQLite + 各 agent 原生 session                    |
| 配置   | 直接读写 `~/.pi/agent/*`                      | 受控安装、Runtime 绑定、Agent Config              |

因此：**pi-web 的“产品能力”值得搬；pi-web 的进程内架构、Next.js/PWA 外壳、文件式状态存储不要搬**，应按 Agent Web 的 Adapter + SQLite 重写。

## 1. 能力对照矩阵

图例：✅ 已有 · 🟡 部分/弱 · ❌ 缺失

### A. 会话与消息

| 能力                                     | Agent Web                      | Pi Web                  | 备注                      |
| ---------------------------------------- | ------------------------------ | ----------------------- | ------------------------- |
| 会话列表                                 | ✅ 按项目分组（worktree 归并） | ✅ 按项目/worktree 分组 | 项目重命名/手动排序仍缺   |
| 置顶                                     | ✅                             | ✅                      | 两者都有                  |
| 归档                                     | ❌                             | ✅                      | pi-web 有 archive + Undo  |
| 重命名                                   | ✅                             | ✅                      | —                         |
| 删除                                     | ✅                             | ✅（含子会话重挂）      | pi-web 会处理后代         |
| 搜索                                     | 🟡 前端本地过滤                | ✅ 服务端全文搜索       | `sessions/search`         |
| 导出                                     | 🟡 Markdown                    | ✅ 自包含 HTML          | —                         |
| Fork 独立会话                            | ✅ `create_pi_fork`            | ✅                      | 语义一致                  |
| **会话内分支切换**                       | ❌                             | ✅ BranchNavigator      | 基于 leafId/navigate_tree |
| **Edit from here（会话内分叉）**         | ❌                             | ✅                      | 复用已解析的 parentId 树  |
| 自动命名                                 | ❌                             | ✅ 调模型生成标题       | —                         |
| **会话统计（tokens/cost/context/时长）** | ❌                             | ✅ header 与列表行      | 需要解析 usage            |
| compaction 折叠展示                      | ❌                             | ✅                      | jsonl `compaction` 节点   |

### B. 转录渲染

| 能力                        | Agent Web                                | Pi Web                                          | 备注                 |
| --------------------------- | ---------------------------------------- | ----------------------------------------------- | -------------------- |
| Markdown/GFM/代码高亮       | 🟡 自研轻量渲染                          | ✅ react-markdown + Prism                       | 自研是为离线，可保留 |
| KaTeX 数学                  | ❌                                       | ✅                                              |                      |
| Mermaid                     | ❌                                       | ✅（懒加载 + 降级）                             |                      |
| 前置 matter / 表格          | ❌                                       | ✅                                              |                      |
| **thinking 内联折叠**       | 🟡 独立 Activity 面板，扁平、按 run 重置 | ✅ 消息内联、可折叠、持久                       | 最大体验差距         |
| **tool call 参数/结果卡片** | 🟡 扁平 label/detail                     | ✅ 参数表、patch diff、文件变更、图片、codemode |                      |
| 工具结果图片                | ❌                                       | ✅                                              |                      |
| 引用选区/分支引用           | ❌                                       | ✅                                              |                      |
| 滚动 minimap                | ❌                                       | ✅                                              |                      |
| turn 时长 / 缓存未命中提示  | ❌                                       | ✅                                              |                      |

### C. 文件与工作区

| 能力                                       | Agent Web                  | Pi Web                          | 备注                       |
| ------------------------------------------ | -------------------------- | ------------------------------- | -------------------------- |
| 文件树 / Git diff                          | ✅                         | ✅                              | —                          |
| @ 提及文件                                 | ✅                         | ✅                              | —                          |
| 文件上传 / 拖拽                            | ✅ 拖拽/选择上传→@引用     | ✅                              | 单文件名，同目录原子重命名 |
| **富预览（图片/音频/PDF/DOCX/CSV 表格）**  | ❌ 明确不支持二进制        | ✅                              | 受离线依赖约束，见 §4      |
| **Git worktree 创建/切换/删除 + 侧栏分组** | ✅                         | ✅                              | 项目重命名/手动排序待做    |
| cwd 浏览/校验/新建目录                     | ✅ 浏览 + 新建（校验隐含） | ✅ `cwd/browse`、`cwd/validate` |                            |
| open in file manager                       | ❌                         | ✅（仅 loopback）               |                            |
| 越界目录只读链接                           | ❌                         | ✅                              | 安全增强                   |

### D. 配置 / 运行时

| 能力                                 | Agent Web                                 | Pi Web                     | 备注                                       |
| ------------------------------------ | ----------------------------------------- | -------------------------- | ------------------------------------------ |
| Agent 安装 / Node Runtime            | ✅（强项）                                | ❌ 不需要                  | 保留 Agent Web 方案                        |
| 模型选择                             | ✅ 从 agent 读取                          | ✅                         |                                            |
| 推理等级（thinking level）           | ✅ 全局默认 + 会话内可改                  | ✅                         | 经 `--thinking` 传给 Pi                    |
| models.json 编辑 / 默认模型          | 🟡 默认模型已有；models.json 仍靠文件编辑 | ✅                         | 不重写 pi 配置                             |
| Provider 登录 / API Key / OAuth      | 🟡 只读状态检查                           | ✅ /login、/logout、写凭据 | 不写 pi 的 auth.json，只读 `pi auth check` |
| 模型测试 / 发现 / 刷新               | ❌                                        | ✅                         |                                            |
| Provider 用量配额                    | ❌                                        | ✅                         |                                            |
| MCP 管理（增删启停/Test/OAuth）      | ✅ 列表/添加/移除/登出                    | ✅ 完整                    | 只调 `pi mcp`，mcp.json 归 Pi 所有         |
| Skills 管理 / 搜索 / 安装            | ❌                                        | ✅                         | 安装需走 Agent Web 受控安装                |
| Plugins 管理 / 更新检查              | ❌                                        | ✅                         | 同上                                       |
| Subagents 配置 / 查看                | ❌                                        | ✅                         |                                            |
| 工具设置（预设/codemode/PowerShell） | ❌                                        | ✅                         |                                            |
| 项目信任（project trust）            | ❌                                        | ✅                         | MCP 前置                                   |
| General 设置（缓存提示/侧栏/外观）   | ❌                                        | ✅                         |                                            |

### E. 客户端 / 平台

| 能力                       | Agent Web                  | Pi Web                          |
| -------------------------- | -------------------------- | ------------------------------- |
| Terminal                   | ✅ xterm（`/terminal/ws`） | ✅（含移动端扩展键、独立主题）  |
| PWA manifest               | ❌                         | ✅                              |
| Web Push 通知              | ❌                         | ✅                              |
| 完成提示音                 | ❌                         | ✅                              |
| 主题 / 字体偏好            | ❌                         | ✅                              |
| i18n                       | ❌ 硬编码中文              | ✅ en / zh-CN / zh-TW / ja / ru |
| 键盘快捷键                 | 🟡 Enter/Esc               | ✅ Esc 停止、Ctrl+Alt+N 等      |
| 移动端适配                 | 🟡                         | ✅                              |
| 多标签页（新标签打开会话） | ❌                         | ✅                              |

### F. 认证 / 安全

| 能力                | Agent Web                 | Pi Web                            |
| ------------------- | ------------------------- | --------------------------------- |
| 浏览器登录          | ✅ 用户系统               | ✅ `PI_WEB_PASSWORD` + Basic Auth |
| 登录限流            | ✅ `005_login_rate_limit` | ✅                                |
| 文件访问 allow-list | ✅ workspace 边界         | ✅ allowed roots + linked dir     |
| 只读会话的 MCP 策略 | ❌                        | ✅                                |
| 请求来源校验        | 🟡                        | ✅ `request-security.ts`          |

## 2. 移植优先级

### P0 — 直接提升“用 pi 的体验”，且与现有架构契合

1. **结构化转录**：把 `thinking / toolCall / toolResult / compaction` 从 jsonl 解析出来并内联到消息流（替代当前丢弃 + 扁平 Activity）。
   - 后端：`backend/src/agents/pi_transcript.rs`（保留树解析，扩展 part 解析）；`session_events.rs`/SQLite 记录可继续兜底实时事件。
   - 前端：`types.ts` 消息模型结构化；`ChatPanel.tsx` + 新组件渲染 tool 卡片/thinking 折叠。
2. **会话内分支**：BranchNavigator（切换 leaf）+ Edit from here（以 entry id 分叉）。
   - 依赖 P0.1 已解析的 entry id/leaf；发送命令经 `process.rs` Pi 分支。
3. **会话统计**：header 展示 tokens / cost / context window / 本 turn 时长。
   - 后端解析 `message.usage`；前端 `ChatPanel` header。
4. **自动命名**：首轮后用模型生成标题（pi-web 的 `session-title.ts` 逻辑），或退化为基于首条用户消息的截断。

### P1 — 工作区与配置能力

5. 富文件预览（图片/PDF/CSV 优先，DOCX/音频次之），受 §4 约束。
6. Git worktree 创建/切换/删除 + 侧栏按项目分组。
7. cwd 浏览/校验/新建目录（`cwd/browse`、`cwd/validate`）。
8. 模型配置：models.json、默认模型/推理等级、enabledModels、context window。
9. Provider 登录 / API Key / OAuth。
10. 文件上传/拖拽 + 工具结果图片。

### P2 — 生态与平台

11. MCP 管理 UI（含 Test、OAuth、项目信任）。
12. Skills 管理/搜索/安装。
13. Plugins 管理/更新检查。
14. Subagents 配置/查看。
15. 工具设置（预设/codemode）。
16. i18n。
17. PWA + Web Push + 完成音 + 主题/字体偏好。
18. 归档、服务端全文搜索、项目分组排序。
19. 多标签页、minimap、快捷键补全。

## 3. 明确不建议移植

- pi-web 的进程内 `AgentSession` / RPC / SSE 架构 → 保留 Agent Web 的进程外 Adapter + WebSocket。
- Next.js SSR / PWA 路由外壳 → Agent Web 是 Vite SPA + Rust 嵌入静态资源。
- `pi-web-session-state.json` 文件式 UI 状态 → 落 SQLite（已有 `session_pinning` 等迁移）。
- `npx skills add` / npm 直接装插件 → 走 Agent Web 受控安装 + Node Runtime PATH 注入（安全设计 §21）。
- pi-web 的 `PI_WEB_PASSWORD` 认证 → 已有用户系统。

## 4. 移植时的硬约束

- **离线/依赖洁癖**：前端目前零 markdown 依赖、自研高亮。引入 KaTeX/Mermaid/PDF/DOCX 会显著增大产物，需逐个评估（可懒加载 + 可选依赖）。
- **二进制预览**：`workspace.rs` 目前对二进制返回“不支持”，需新增按 mime 的流式返回与 allow-list 校验。
- **进程外模型**：任何需要“读 pi 配置 / 调模型”的 UI（模型配置、自动命名、用量）都要经后端 adapter，不能在浏览器直连。
- **Makefile 优先**（项目约定），改完后必须 `prek run -a` 通过：新增 `prek.toml` 只用 local system hook 转调 `make fmt` 与 `make check`，不复制命令；`make lint`/`make test` 留给 CI（固定 Rust 1.88，本地新工具链有历史遗留报错）。

## 5. 建议的第一步

先做 **P0.1 结构化转录**：它是 P0.2（分支）、P0.3（统计）、P1.5（工具图片）的前置，收益最直接，且落点集中在 `pi_transcript.rs` + 前端消息渲染。

## 6. 迁移进度

### ✅ P0.1 结构化转录（已完成）

- 后端 `backend/src/agents/pi_transcript.rs`：`TranscriptMessage` 新增 `parts`（`text`/`thinking`/`toolCall`）、`tool_call_id`、`is_error`、`usage`；新增 `toolResult` 消息与 `compaction` 摘要消息；仅 `text` 的旧 `content` 字段保留，向后兼容。
- 前端 `frontend/src/types.ts`、`frontend/src/components/MessageParts.tsx`、`ChatPanel.tsx`：思考折叠、工具调用卡片、工具结果卡片、压缩提示条。
- 完成一轮对话后重新拉取 `/sessions/:id/messages`，用结构化转录替换流式纯文本（非 Pi agent 返回空列表，不受影响）。

### ✅ P0.3 会话统计（已完成）

- `message.usage` 随转录返回；`frontend/src/components/SessionStats.tsx` 汇总上下文 tokens、累计 tokens、费用、轮次，显示在顶栏。

### ✅ P0.4 自动命名（已完成）

- `backend/src/api/session_messages.rs`：首条用户消息截断 60 字作为会话标题（回退到时间戳命名），与 Pi 原生命名习惯一致。

### ✅ P0.2 会话内分支切换（已完成，采用方案 1）

pi CLI 未暴露 `navigate_tree` / `--branch` / `--leaf`，只有整会话级 `--fork <session>`；真正的“从某条消息分叉 / Edit from here”通过 pi RPC 模式实现，Agent Web 不重写 Pi 的会话树：

- 后端 `backend/src/agents/pi_rpc.rs`：一次性短生命周期 RPC 客户端（`pi --mode rpc --session <id>`），`fork {entryId}` → 再 `get_state` 取得新会话 id。必须先发 `fork` 再问 `get_state`（反之拿到的是源会话），读取期间保持 stdin 打开，60s 超时，子进程随 drop 终止。
- 后端 `backend/src/api/sessions.rs`：新增 `POST /sessions/{id}/fork-from`（`{entry_id}`）→ `{session, text}`。先本地校验：非 Pi / 运行中 / 原生文件缺失 409，空 entry 或非用户消息 400（Pi 只能从用户消息分叉）；Pi 侧报错 502。分叉发生在首条消息之前时 Pi 不落盘 → `native_session_id` 存 `NULL`，下一条消息会新建原生会话（此时源会话仍为 idle）。
- 前端 `frontend/src/components/ChatPanel.tsx`：用户消息摘要行新增「从此分叉」（二次确认），成功后跳到新会话并把该条消息放回输入框等待改写；顶部原有「创建分支」保留为整会话复制。`useSessionActions.forkFromEntry` + `useAppController.forkFromMessage` 串联。
- 验证：`pi --session <forked id> get_state` 返回的 `sessionFile` 指向新文件、`messageCount` 等于分支点之前的消息数，说明新会话可直接续跑。

### ✅ P1.5 富文件预览（图片/音频/PDF/CSV，已完成）

- 后端 `backend/src/api/workspace.rs`：新增 `GET /sessions/:id/raw/{*path}`，按扩展名 allow-list 返回真实 MIME 的原始字节（`image/*` 光栅格式、`audio/*`、`application/pdf`），供 `<img>`/`<audio>`/`<iframe>` 内联渲染；沿用与 JSON 预览相同的路径规范化、逐级 symlink 拒绝与根目录边界校验。大小上限 25 MiB，`nosniff` + `no-store`；未列入 allow-list 的类型返回 415。
- 前端 `frontend/src/components/RichPreview.tsx`：按扩展名分派到图片/音频/PDF 渲染；CSV/TSV 用自研小解析器渲染为表格（500 行 × 50 列上限，含引号转义），零新增依赖。
- **未做**：DOCX 文本抽取需要 zip/inflate 解析，收益低且会引入依赖，暂缓（`ponytail:` 记录）。

### ✅ P1.10 工具结果图片（已完成）

- 后端 `backend/src/agents/pi_transcript.rs`：`TranscriptPart` 新增 `mime_type`、`image_index`，解析 `toolResult`/`user` 内容数组里的 `image` 部分；新增 `read_entry_image`（含 12 MiB base64 上限与光栅 MIME allow-list）+ 自带 base64 解码器（无新依赖），按 `entryId`+part 下标取回字节。
- 后端 `backend/src/api/session_messages.rs`：新增 `GET /sessions/:id/image/{entry_id}/{index}`，走同一转录读取限流（信号量 4），base64 不进入消息列表。
- 前端 `frontend/src/components/MessageParts.tsx`：工具结果卡片懒加载展示图片，折叠头显示图片数量。

### ✅ P1.9 推理等级 structured 配置（已完成）

- 后端：`migrations/012_session_thinking.sql` 给 `sessions` 加 `thinking_level`；`agents/adapter.rs` 新增 `THINKING_LEVELS` + `normalize_thinking_level()`（白名单 `off/minimal/low/medium/high/xhigh/max`，未知值拒绝）；`AgentConfig.thinking_level` 在 `agents/session.rs` 从会话注入，`agents/commands.rs` 的 Pi 分支在有值时追加 `--thinking <level>`。
- API：`POST /sessions` 接受 `thinking_level`（非法值 400）；新增 `PUT /sessions/{id}/thinking`（`null`/空串清除，回退到 Pi 自身配置的默认），运行中 409；`set_session_agent` 与 fork 同步重置/复制该列。
- 前端：新增 `ThinkingSelector.tsx`（复用 `model-selector` 样式），仅在 Pi Agent 的会话顶部显示；`types.ts` 导出 `THINKING_LEVELS`；`useSessionActions.setThinking` + `useAppController.changeThinking` 接入 `ChatPanel`。
- 测试：`normalizes_known_levels_and_rejects_unknown`、`pi_thinking_level_is_forwarded_only_when_set`。
- **未做**：每个模型的 thinking 默认值持久化（π-web 的 star 保存默认），models.json 的结构化编辑器仍靠 AgentConfig 文件编辑。

### ✅ P1.11 Provider 凭据状态（只读，已完成）

- 后端：`agents/models.rs` 新增 `check_auth()`（调 `pi auth check --provider <p> --json --no-refresh`，容忍非零退出码，支持 JSON 与旧版纯文本两种输出）+ `valid_provider_name()`（防把值当 flag 传入）+ `parse_auth_report()`。`command_output` 重构为 `command_output_status`（返回 success/stdout/stderr），两者共用。
- API：`GET /agents/{id}/auth?provider=<p>` → `{provider,status,auth_type?,reason?}`；非 Pi Agent 返回 501，非法 provider 名 400，命令失败 502。
- 前端：`AgentConfigDialog.tsx` 新增「Provider 凭据」区（仅 Pi）：点「检查」时拉 `/agents/{id}/models` 取去重 provider，再并发查各 provider 状态，渲染 ready/not_ready/invalid 徽章 + authType/reason。
- 测试：`parses_both_pi_auth_report_shapes`、`rejects_provider_names_that_could_be_flags`。
- **未做（有意）**：不写 `auth.json`（未公开格式，交回 Pi 自己的 `/login`）；不在 UI 里登录 OAuth（需要 pi SDK 的浏览器/设备流，属于重新实现 Agent Runtime）；`print-api-key` / `print-bearer-token` 不接（会向浏览器回传密钥）。

### ✅ P1.8 文件上传 / 拖拽（已完成）

- 后端 `backend/src/api/workspace.rs`：新增 `POST /sessions/{id}/upload?name=file[&dir=sub]`。`name` 必须是单个普通路径分量（拒绝空名、`.`、`..`、`/`、绝对路径）；`dir` 可选、复用 `workspace_file_path` 的逐级 symlink 拒绝 + 根边界校验；目标已存在（文件/目录/软链）返回 409 不覆盖；上限 25 MiB（413）。写入采用同目录暂存文件 + `rename` 原子落盘，失败清理暂存，不会是半截文件。返回 `{path, size}`（工作区相对路径）。
- 前端 `frontend/src/components/ChatPanel.tsx`：新建会话区支持拖拽文件（全屏虚线提示）与回形针选择文件，上传成功后自动在输入框追加 `@文件名`（含空格用 `@{...}`）引用；`types.ts` 新增 `UploadedFile`。
- 测试：`upload_name_must_be_a_single_normal_component`（`api::workspace` 共 8 个测试通过）。
- **未做（按需再加）**：目录树内拖放到指定子目录、覆盖/跳过冲突策略、超大文件流式分片。

### ✅ P1.7 cwd 新建目录（已完成）

- 后端 `backend/src/api/sessions.rs`：新增 `POST /workspaces {parent?, name}`，把父目录 canonicalize 并限定在 `AGENTWEB_WORKSPACE_DIR` 内，再调用已有的 `resolve_workspace_under`（逐级创建、逐级 symlink 拒绝）返回规范化路径。该函数原先只有测试没有入口，现已接入路由，消除了闲置告警。
- 前端 `frontend/src/components/ChatSelectionDialog.tsx`：目录选择器底部新增“新建文件夹”输入 + 按钮，回车即创建并进入该目录（同时设为新会话工作目录）。
- **未做**：独立 `cwd/validate` 端点（浏览与创建已隐含校验，暂不需要）。

### ✅ P1.6 Git worktree 浏览/创建/删除 + 侧栏按项目分组（已完成）

- 后端 `backend/src/api/worktrees.rs`：新增 `GET /worktrees?cwd=…`、`POST /worktrees {cwd, branch}`、`DELETE /worktrees {cwd, path, force}`。
  - `cwd` 先 canonicalize 并校验落在 `AGENTWEB_WORKSPACE_DIR`（默认 `$HOME`）之内，与创建会话同一边界；仓库根由 `git rev-parse --git-common-dir` 推导。
  - 列表解析 `git worktree list --porcelain`，跳过 prunable 并标记目录是否仍存在；`is_main` 取第一项。
  - 创建位置固定为 `<repoRoot>-worktrees/<sanitized-branch>`，已存在分支则复用、否则 `-b` 从当前 HEAD 新建（不 fetch，保持离线安全）；git 命令均带超时且 `kill_on_drop`，创建后再做一次 `canonicalize` 越界校验，越界则回滚删除。
- 前端 `frontend/src/components/ChatSelectionDialog.tsx`：在工作目录选择器下方新增 Git Worktree 区块——列出当前仓库的 worktree（分支/主/缺失标记），点击即把该 worktree 设为新会话工作目录（即“切换”）；输入分支名可直接新建 worktree 并自动选中；选中非主 worktree 时可删除，`409/WORKTREE_DIRTY` 时二次确认强制删除（对应 pi-web 的 `worktreeRemovalRequiresForce`）。零新增依赖。
- 前端 `frontend/src/components/Sidebar.tsx`：会话列表改为按项目分组——`<repo>-worktrees/<branch>` 归并回主仓库，置顶会话单独成组；分组可折叠且折叠状态存 localStorage。纯前端，无新请求。
- **未做**：项目重命名/手动排序（pi-web 的 `sidebar-prefs`）。

### ✅ P2.1 MCP 管理（列表/添加/移除/登出，已完成）

- 原则：Agent Web 自己不说 MCP，只包一层 `pi mcp`，`mcp.json` 始终归 Pi 所有（与 §3 的“不重写 pi 配置”一致）。
- 后端 `backend/src/agents/mcp.rs`：
  - `list()` = `pi mcp list --json`（超时 60s）。注意 `list` 会**逐个连接**所有启用的服务器：`state` 为 `connected` / `failed`，失败的服务器带 `error`（如 `fetch failed`、`spawn … ENOENT`），此时 `pi` 退出码为 1 —— 因此以“能否解析出 JSON”判成败，不看退出码。`McpServer` 除 `name` 外全部可选，单个残缺条目不会让整个列表失败。
  - `McpAddRequest::arguments()` 组装 `pi mcp add` 的 argv：名称走 `valid_cli_token`；URL 与命令必须二选一；`--exposure` 白名单 `codemode/deferred/direct/hidden`；`--header` 只接受 `KEY=VALUE` 且拒绝 `Authorization`（否则密钥会明文落进 `mcp.json`，引导用户改用 `--bearer-token-env-var`，Pi 会写成 `Bearer ${ENV}`）；参数一律作为独立 argv 元素传入（`pi mcp` 不认 `--flag=value`）。
  - 每个写操作（`add`/`remove`/`logout`）执行后立即重跑 `list()` 并把最新视图返回给前端，避免“写完还要再点一次刷新”。
- API `backend/src/api/mcp.rs`：`GET /agents/{id}/mcp`、`POST /agents/{id}/mcp`、`DELETE /agents/{id}/mcp?name=`、`POST /agents/{id}/mcp/logout?name=`；非 Pi Agent → 501；请求体非法 → 400；Pi 自身报错（例如对 stdio 服务器执行 logout）→ 400 + 原始消息。
- 前端 `frontend/src/components/McpPanel.tsx`（嵌在 `AgentConfigDialog` 的 Pi 分区，点「加载」才连服务器）：列出名称/state 徽章/transport/exposure/工具数/错误，每行可「登出」「移除」（移除二次确认）；下方是添加表单（名称 + 命令|URL + 空格分隔参数）。
- 同时把 `mcp.json` 加入 Pi 的 `known_paths`，因此需要请求头、OAuth、环境变量等 UI 未覆盖的字段时，可直接用同一个对话框里的文件编辑器改。
- 测试：`builds_stdio_arguments_with_a_double_dash_separator`、`builds_http_arguments_and_keeps_secrets_out_of_mcp_json`、`rejects_inputs_that_could_be_read_as_flags`、`parses_pi_mcp_list_output`。
- 端到端验证（临时 DB 副本 + 假 stdio MCP server + 一个坏 URL + 一个不存在的二进制）：`GET` 正确区分 connected/failed 并带回 `fetch failed` / `spawn … ENOENT`；`POST` 添加后立刻出现在列表且 `state: connected`；`Authorization` 请求头、`--` 开头的名字、同时给 URL 与命令均返回 400；非 Pi Agent 返回 501；对 stdio 服务器 logout 返回 Pi 的原始提示；`DELETE` 后 `mcp.json` 同步移除。
- **未做（有意）**：OAuth 登录（`pi mcp login`）不接 UI —— 它是浏览器/设备码流程，属于重新实现 Agent Runtime；`--env`/`--header`/`--cwd`/`--oauth-client-id` 等表单字段未做（用文件编辑器或 `pi mcp` 即可）；启停（enabled）开关与工具 Test 未做。

### ✅ P2.2 技能 / 提示模板 / 扩展命令列表（已完成）

- 原则：技能、模板、扩展命令全部由 Pi 自己发现（settings、packages、项目祖先目录、信任状态），Agent Web 只做只读展示，不解析 `SKILL.md`、不复制安装逻辑。
- 后端 `backend/src/agents/pi_rpc.rs`：在原有一次性 RPC 客户端上抽出 `RpcSession`（start/send/read/stop），新增 `list_commands()`——`pi --mode rpc --no-session --no-mcp` + `get_commands`，不建会话文件、不连 MCP 服务器，约 1.5s 返回。`CommandInfo {name, description, source, scope, path, origin, base_dir}`，逐个条目容错（缺 name 或畸形条目直接跳过，不会因一条坏数据失败）。
- API `backend/src/api/agents.rs`：`GET /agents/{id}/commands?cwd=<dir>`。非 Pi 501；`cwd` 可选，复用 `worktrees::allowed_cwd` 的 canonicalize + 根边界校验（工作区外 403、不存在 404），因此项目级资源只会在允许的目录里被看到；Pi 报错 502 + stderr。
- 前端 `frontend/src/components/SkillsPanel.tsx`（`AgentConfigDialog` 的 Pi 分区，打开即加载）：按 `skill`/`prompt`/`extension` 分组（含计数徽章）、本地过滤（名称/描述/路径）、显示 scope（用户/项目/临时）与「包」标记、展示 SKILL.md 路径；并提示“在会话中用 /名称 触发；项目级资源需先在 pi 中信任该目录”。
- 验证：`GET /agents/pi/commands?cwd=<repo>` 返回 69 条（62 skill + 7 extension，含 user/project scope 与 origin）；无 `cwd`、非 Pi（501）、`/etc`（403）、不存在目录（404）、无 cookie（401）均符合预期；`parses_commands_and_skips_malformed_entries` 单测覆盖畸形条目与失败响应。
- **未做（有意，与 §3 一致）**：不接 `npx skills add` / 市场安装 —— 装什么由用户自己的 Pi 决定；不编辑 `SKILL.md`（用同一个对话框的文件编辑器即可）。

### ✅ P2.3 会话内容搜索（服务端，已完成）

- 背景：侧栏原有的搜索只能过滤标题/工作区（标题就是首条用户消息），找“那个改过 X 的会话”需要搜对话正文。Pi 的对话正文在原生 JSONL 里，不在 SQLite（`messages` 表早已不用），所以必须由后端读会话存储。
- 后端 `backend/src/agents/pi_transcript.rs`：新增 `search_transcripts(ids, needle, budget)`（内部 `search_transcripts_in_root` 便于测试）。一次遍历建 `<session id> → 文件路径` 索引（文件名 `_<id>.jsonl`，不打开文件），然后按“新→旧”顺序逐会话读取：
  - 逐项上限：最多 300 个文件、30 条结果、单文件 16 MiB、总预算 3s；命中上限即返回 `truncated: true`，表示“旧会话未搜到”而非“无匹配”。
  - 文件名的 id 不算数，头部 `type/id` 必须匹配（避免残留/伪造文件名回答别的会话）；读取失败标记为不完整。
  - 匹配为字面量大小写不敏感（`find_ci`：小写化后定位，再回到原串的字边界）；只搜 `user`/`assistant` 文本，每条会话取首条命中，返回 `before/matched/after` 上下文片段（各 60 字符，空白折叠、截断处加“…”）。
- API `backend/src/api/sessions.rs`：新增 `GET /sessions/search?q=<关键词>` → `{results:[{session, entry_id, role, before, matched, after}], truncated}`。关键词去空白后不可为空、最多 200 字符（否则 400）；候选会话按 `updated_at DESC` 取 500 条，复用 `TRANSCRIPT_READ_LIMIT` 信号量 + `spawn_blocking`，不阻塞异步执行器。
- 前端 `frontend/src/components/Sidebar.tsx`：搜索框输入 300ms 防抖后请求后端，在会话树上方显示「内容匹配」结果区（标题 + 高亮片段 + `truncated` 提示），点击直接打开该会话；原有标题/工作区即时过滤保留，两者互补。`types.ts` 导出 `SessionSearchHit` / `SessionSearchResponse`。
- 测试：`searches_transcripts_across_sessions_and_reports_snippets`（跨会话命中、大小写不敏感、头部 id 不匹配的会话被跳过）、`finds_matches_on_character_boundaries`（多字节 snippet 窗口与边界）、`reads_the_session_id_out_of_a_transcript_filename`。
- 端到端验证：临时 DB + 会话存储下，`?q=second` 命中用户消息并带回 `entry_id: cccc3333` 与片段，`?q=SECOND` 同样命中，无匹配返回空且 `truncated:false`，空关键词/超长关键词均 400。
- **未做（有意）**：不建全文索引（`ponytail:` 已标注，待实测延迟不够再加速）；不做“跳到具体消息并高亮”（SQLite 消息与 Pi entry 不同源，需端到端对齐 entry 位置，收益低）。

### ✅ P2.4 会话归档（已完成）

- 迁移 `013_session_archive.sql`：`sessions.archived_at TEXT`（`NULL` = 未归档）。`Session` 结构新增 `archived_at`，所有查询会话的地方（`list_sessions`、`load_session_record`、搜索、删除前置检查、调度器）同步投影该列。
- API：`PUT /sessions/{id}/archive`，体为 `{"archived": true|false}` — 存显式值（非 toggle），取消归档幂等；归档写入当前时间戳，同时更新 `updated_at`。校验：不存在 404、未登录 401、缺字段 422。
- 前端：
  - `useSessionActions.setArchived` / `useAppController.toggleArchive`（无确认弹窗，归档只是换个分组，返回路径就在归档组里）。
  - `Sidebar.tsx`：归档会话从项目分组（含置顶）中移出，统一落到末尾的「归档」组（`ARCHIVED_KEY`，图标 `Archive`，首次默认折叠，折叠状态与项目组共享 `agentweb.sessionGroups` 存储）；菜单新增「归档 / 取消归档」；行图标随状态切换；内容搜索结果标题旁标「· 已归档」。
- 验证：临时 DB 上 `PUT archive {true}` → `archived_at` 写入、列表与搜索结果均带回该字段，`{false}` → `null`；404/401/422 均符合预期；前端产物包含「归档 / 取消归档 / 已归档」字样。
- **与 pi-web 的不同（有意）**：pi-web 按“家族（root + subagents）”归档、以 `archivedAt >= 会话文件 mtime` 在会话有新消息时自动移除归档；Agent Web 的每个侧栏行就是一个独立会话（fork 是平级而非子代理），因此归档按单个会话生效，并且不做自动取消归档（Agent Web 不逐条消息跟踪文件 mtime）。

### ✅ P2.5 插件（Pi 包）管理（已完成）

- 背景：pi-web 的 Plugins 面板直接在进程内用 SDK 的 `DefaultPackageManager` / `SettingsManager` 解析包、过滤资源、读写配置 —— 那是 Agent Runtime 的内务，Agent Web 不做。对应做法是薄封装 pi 自己的 CLI：`pi list` / `pi install` / `pi remove` / `pi update`；`settings.json` 与已安装目录都归 pi。
- 后端 `backend/src/agents/plugins.rs`：
  - `list`：跑 `pi list`，解析它的文本报告（`User packages:` / `Project packages:` 段落；2 空格缩进行是来源，4 空格缩进行是可选的已安装路径，`No packages installed.` 即空列表）。
  - `install` / `remove`（项目级加 `-l`）/ `update`（无来源 = `--extensions` 全部，有来源 = `--extension <来源>`），每次变更后重新 `list` 返回新状态。
  - 来源校验只做一件事：非空、不超 512 字符、**不以 `-` 开头**、无控制字符 —— 它作为单个 argv 传入，绝不能被当作 flag；来源文法（npm scoped、`git:`、ssh URL、本地路径）归 pi 判定。
  - 超时 300s（安装/更新会拉 npm/git）；不复现 pi-web 的“按资源过滤启用/禁用单个资源”与“检查 npm 更新”（后者要自己解析 npm registry 与 semver）。
- API `backend/src/api/plugins.rs`：`GET /agents/{id}/plugins?cwd=`、`POST /agents/{id}/plugins?cwd=`（`{action: install|remove|update, source?, local?}`）。非 Pi 501；`cwd` 复用 `worktrees::allowed_cwd`（越界 403、不存在 404）；`-` 开头来源与缺来源/非法动作均 400；pi 自身的报错（如“Project is not trusted”）原样透出为 502。
- 前端 `frontend/src/components/PluginsPanel.tsx`：在 Agent 配置弹窗（仅 Pi）中，与 MCP / 技能面板并列；列表显示来源、作用域徽标（用户/项目）、安装路径，逐项「更新」「移除」，头部「更新全部」，底部按来源安装并可选「安装到项目」。项目级条目需先在 pi 中信任目录（与技能列表同一规则，Agent Web 不会替你传 `--approve`）。
- 测试：`parses_both_scopes_and_an_optional_path`、`rejects_a_source_that_could_be_read_as_a_flag`。
- 端到端验证：`?cwd=<已安装工作区>` 列出 ponytail（user + 路径）；未信任的项目目录只返回用户级包（隐藏项目级）；codex 501、`/etc` 403、不存在目录 404、`--self` 400、缺来源 400、未知动作 400、移除不存在包 502（透出 pi 原文）、未登录 401；`{"action":"update"}` 3.4s 内返回更新后的列表。

### ✅ P2.9 引用选区 + 新标签打开会话（已完成，纯前端）

- **引用选区**（对应矩阵 B“引用选区/分支引用”）：`ChatPanel.tsx` 监听 `selectionchange`，只在选区落在转录区（`chatRef`）、且不在 textarea/input 内时，才在输入框左侧出现引用按钮；点击后逐行加 `> ` 前缀插到输入框最前面，然后清掉浏览器选区。无后端、无依赖。
- **新标签打开会话**（对应矩阵 E“多标签页”）：会话菜单加一项，`window.open(sessionPath(id), "_blank")`。深层链接本来就成立（`/sessions/{id}` 由 `serve_embedded_frontend` fallback 到 index.html），所以不需要新路由。
- 有意不做：页内多 tab 标签栏（深层链接 + 浏览器标签页已经覆盖这个需求）、“在新标签打开”的服务端校验。

### ✅ P2.8 偏好（完成提示音，已完成）

- 背景：pi-web 的偏好面板含主题 / 字体 / 完成音 / PWA / Web Push。这里只拿“一轮回复结束提醒一声”，因为它是唯一浏览器平台没提供能力的那个：
  - **界面缩放不做**：整站 1189 处 px、仅 1 处 rem，而 `zoom` 与 `100dvh`/`100vw` 混用是否溢出在无浏览器环境无法验证；浏览器的页面缩放（Ctrl +/-）本来就能缩放整套 px 样式。
  - **深色主题不做**：543 处写死的十六进制颜色，只有 118 处已用 `--ui-*` 设计变量；换主题等于整体改造现有样式表。
  - **PWA / Web Push 不做**：需要 service worker、manifest 与 VAPID 密钥，而部署形态是 Docker 内部工具。
- `frontend/src/lib/preferences.ts`：纯函数 `parsePreferences(raw)` —— 存档坏了/非布尔值一律退回默认（可在 node 里断言）。
- `frontend/src/hooks/usePreferences.ts`：`agentweb.preferences` 存 localStorage（与 `agentweb.sessionGroups` 一致，不进数据库）；`playCompletionSound()` 用 Web Audio 合成两声，不引入音频资源；`useCompletionSound(enabled, runningIds)` 看的是**整个会话列表的运行集合**（切到别的会话或标签页也能听见），从运行变空闲才响。
- `frontend/src/components/PreferencesDialog.tsx`：侧栏底部「偏好」入口，面板含开关与「试听」。
- 检查：`scripts/check-frontend.mjs` 增加 6 个偏好存档断言（`null`/非法 JSON/数组/字符串布尔/true/false）；`make test` 现在一次编译 `sessionFamily.ts` + `preferences.ts` 并跑同一个检查脚本。

### ✅ P2.7 会话工具权限（工具预设，已完成）

- 背景：对应 pi-web 的“工具设置（预设/codemode）”。pi-web 用 `PRESET_READ_ONLY` 这类工具预设控制子代理能干什么，而 pi 本身就有会话级工具开关（`--tools` / `--exclude-tools` / `--no-tools` / `--no-builtin-tools`，对应 settings.json 的 `defaultTools`）。所以这件事不需要重写工具运行时，只把预设转成 pi 自己的参数。
- 预设取值（后端白名单校验，同推理等级）：
  - `default`（NULL）—— 不加参数，用 pi 自己配置的 `defaultTools`；
  - `readonly` → `--tools -bash,-edit,-write`（只读）；
  - `nobash` → `--tools -bash`；
  - `notools` → `--no-tools`（连扩展与 MCP 工具一起关）。
- 关键点：`--tools` **只用 `-name` 形式的删除项**，按 pi 的规则这是“在继承的选择上做加减”，所以预设不会把用户自己的 `defaultTools` 整体覆盖掉；工具名与匹配也完全由 pi 解析，Agent Web 不维护工具清单。
- 后端：`migrations/014_session_tools.sql`（`sessions.tool_preset`）；`agents/adapter.rs` 的 `TOOL_PRESETS` / `normalize_tool_preset` / `tool_preset_args`；`agents/commands.rs` 的 Pi 分支只在设置时追加参数；`agents/session.rs` 把它送进 `AgentConfig`；`PUT /sessions/{id}/tools`（`{tool_preset}`，`null` 清除）；创建会话时也可带预设（非法值 400，不再静默忽略）；fork 会继承源会话的预设。
- 前端：`frontend/src/components/ToolPresetSelector.tsx`（与推理等级选择器同形，显示实际的 pi 参数），在会话工具栏中仅对 Pi 显示；`types.ts` 的 `TOOL_PRESETS`；`useSessionActions.setTools` / `useAppController.changeTools`。
- 测试：`normalizes_known_presets_and_rejects_unknown`、`every_preset_maps_to_argv_or_nothing`、`pi_tool_preset_is_forwarded_only_when_set`。
- 端到端验证：`PUT /sessions/e2e-src/tools {readonly}` → 返回并持久化 `readonly`；`{all}` → 400；`{null}` → 清除；不存在会话 404；未登录 401；创建会话带 `nobash` → 200 且写入，带 `wat` → 400，不传 → NULL。
- 有意不做：pi-web 的 codemode 作用域/子代理工具继承（那个运行时是 pi-web 自己的示例扩展，不在 pi 核心里）；也不做自由填写工具名（要自己做一遍 pi 的工具名与 pattern 校验，而预设想表达的场景已经覆盖）。

### ✅ P2.6 会话家族（分支缩进，已完成）

- 背景：pi-web 把“分支/子代理”排成家族树（父会话 + 子会话，根节点归档整族）。但 pi 的子代理只是 `examples/extensions/subagent/` 里的示例扩展，不在 pi 核心里；要复现它的“运行/队列/技能/MCP 继承”等就是重写 Agent Runtime，不做。
- 保留的是真正能站住的那一层：**分支关系本身**。Agent Web 创建 fork 时已经把源会话的原生 id 写进了 `sessions.fork_from_native_session_id`，所以父链在纯前端就能复原，**无需后端改动**。
- `frontend/src/lib/sessionFamily.ts`：纯函数 `familyRows(sessions)` —— 按原生 id 建立 `native id → 会话` 索引，标记有父的会话，然后从根开始 DFS，输出 `{session, depth}`（父在前、分支紧跟、逐层缩进）；自引用/互引用/depth>8 都不会死循环，父会话不在这批（例如父已归档而子还在项目组）也不会丢行。
- `frontend/src/components/Sidebar.tsx`：每个分组内改调 `familyRows(group.sessions)`，depth>0 的行加左侧缩进与 `GitBranch` 图标（归档行仍优先用 `Archive` 图标）。归档分组、置顶分组、项目分组互不干扰，计数不变。
- 有意不做：pi-web 的“按家族归档/取消归档”（Agent Web 是单会话归档，已在 P2.4 说明）、子代理运行时与其配置面板（属于 Agent Runtime 与示例扩展范畴）。
- 检查：`make test` 除 `cargo test` 外，会用 `tsc` 编译 `src/lib/sessionFamily.ts` 后跑 `frontend/scripts/check-session-family.mjs`（6 个断言：父子三级顺序、无关会话保序、无原生 id 的分支、父不在本批、自引用、互引用）。不新增测试框架，也不加进 prek 的 `make check` 门禁。

### ✅ 顺带修复：配置目录与运行时 HOME 不一致

- 发现问题：Agent 实际运行时 `agents/commands.rs` 把 `HOME`/`XDG_*` 设为 `AGENTWEB_AGENT_HOME`（Docker 下恰好等于 `HOME`，所以一直没暴露），但 `models.rs` 的 `pi --list-models`、`pi auth check`、`pi mcp list` 以及 `config.rs` 的 `known_paths` 用的都是服务自身的 `HOME`。只要两者不同（裸机部署），就会出现“编辑器改的文件 Agent 不读、MCP 面板显示的服务器不是实际生效的”这类静默错配。
- 修复：`agents/config.rs` 新增 `agent_home()`（`AGENTWEB_AGENT_HOME` > `HOME`，与 `commands.rs` 同一优先级）作为唯一来源；`pi_agent_dir()` 在其之上再让 `PI_CODING_AGENT_DIR` 覆盖；`models.rs::run_agent_command` 在 `agent_home()` 存在时把 `HOME`/`XDG_*` 注入子进程，使所有内省命令与真实运行看到同一份配置。

### ✅ 浏览器实测（headless Chrome）与随之修掉的 5 个前端崩溃

- 之前每一项的验收都只到“`npm run build` 通过 + 在 bundle 里搜到关键字”，本轮补上真机验证：Chrome for Testing headless shell + `chrome-devtools-mcp`，用独立驱动脚本（`navigate_page` / `evaluate_script` / 真实 CDP 点击）实际点一遍。**结论：之前漏掉了 5 个会让页面直接崩掉或静默失效的 bug。**
- 修掉的崩溃（前 4 个全在 `Sidebar.tsx`）：
  1. **侧栏一个分组都不渲染**：项目 `byKey` 建了 Map，却从没映射成 `groups`（“构建了数据但没渲染”）。现在分组逻辑抽到 `frontend/src/lib/sessionGroups.ts`（`groupSessions` / `PINNED_KEY` / `ARCHIVED_KEY`），可在 `make test` 里跑，回归不了。
  2. `ReferenceError: PINNED_KEY is not defined`（漏导入）。
  3. `ReferenceError: sessionPath is not defined`（漏导入 `app/router`）。
  4. `Cannot find name 'GROUP_STORAGE_KEY'`（删常量时删过头）。
  5. **Pi 专属功能全部静默不渲染**：`/agent-catalog` 的响应里没有 `kind` 字段，而 `useAgentRuntime` 合并“目录 + 已配置”时用 `{ ...known, installed: true }` 覆盖，`kind` 就变成 `undefined`；于是 `currentAgent?.kind === "pi"` 的判断永远为假——推理等级、工具权限、分叉入口、Agent 配置里的 Provider 凭据/MCP/技能/插件面板**一个都不显示**，而且不报错。修法是回填：`{ ...known, kind: agent.kind, installed: true }`。
- 防复发门禁（都不引入新依赖）：
  - `frontend/scripts/check-names.sh`：用 `tsc --noEmit` 只挑 `TS2304/TS2552/TS2306/TS2307`（引用不存在的名字/模块）这类“一加载就崩”的错，接进 `make check`；
  - `frontend/scripts/check-frontend.mjs`：`sessionFamily` / `preferences` / `sessionGroups` 的纯函数断言，接进 `make test`。
- 实测通过的项（同一套驱动，含真实点击）：
  - 侧栏 2282 行会话 + 分组（`agentweb`、`.dotfiles`…）正常渲染；搜索能出命中（19 条 + “只搜索了较新的会话…”提示）；会话菜单 = 置顶/重命名/在新标签打开/归档/删除会话；「在新标签打开」真的开了第二个页面到 `/sessions/{id}`；家族缩进（`padding-left: 30px` + `lucide-git-branch`）与父行相邻。
  - 会话面板 1415 行消息渲染；引用选区按钮存在且把 `> ` 前缀文本插进输入框。
  - Pi 会话顶部四个选择器（AGENT/MODEL/THINKING/TOOLS）齐全：`THINKING 高 --thinking high`、`TOOLS 只读 --tools -bash,-edit,-write` 点完即更新，且 `GET /api/sessions` 回流确认 `thinking_level=high`、`tool_preset=readonly`；「创建分支」+ 每行「从此分叉」按钮都在。
  - Agents & runtimes 弹窗 → Pi「配置」：Provider 凭据（检查）、MCP 服务器（加载，空态文案正常）、命令与技能（62 个技能 + 7 个扩展命令）、插件（Pi 包）1 个真实条目。
  - 偏好：勾选后 `localStorage.agentweb.preferences = {"completionSound":true}`，刷新后仍勾选；Terminal 面板真连上（`Connected` + zsh 提示符）；新建 Chat 弹窗列出 Agent/模型/工作目录。全程控制台无报错。
- 本轮**未**实测（有意跳过）：真正点「从此分叉」落一个分支、发一条消息跑完一轮 Agent（会写真实 pi 会话目录、消耗 token），这两个的后端路径此前已用 API 级 E2E 验过，UI 只验到按钮与请求存在。
- **未复现的一次崩溃**：某次脚本运行里 App 渲染抛 `TypeError: Cannot read properties of undefined (reading 'id')`（被 `AppErrorBoundary` 接住，同时弹了一个同文 alert），随后 9 次重新加载（含 4 次路由往返、8 次整页刷新）都没再出现，服务端日志全是 200、无 5xx。怀疑是渲染期某处未加保护的 `.id`，但没有稳定复现路径，先记录不动代码。

### ✅ 对话界面补全：工具结果元信息、失败轮次、长转录分页

- 先按数据立论，而不是照抄 pi-web 的组件。取样 40 个真实 pi 会话（`~/.pi/agent/sessions`）：**922 条 toolResult 全部带 `durationMs`、0 条含 ANSI 转义**；role 分布 `toolUse 846 / error 162 / stop 38 / length 1`；toolResult 的 `details` 形态为 `diff/firstChangedLine/patch`（42 条，全部来自 edit）、`truncation`（6 条）、`mcpTool`（1 条）。据此只做有数据支撑的三件事。
- **工具结果卡片**（`backend/src/agents/pi_transcript.rs` + `frontend/src/components/MessageParts.tsx`）：`toolResult` 现在额外带 `tool_name` / `duration_ms` / `truncated` / `diff` —— 卡片标题从“工具结果”变成 `bash 44ms`、`edit 9ms 输出已截断`，展开时若有 `details.diff` 就用一个紧凑的统一 diff 片段（`+`/`-`/`@@`/`...` 分色）直接展示改了什么，不用再点开文件对比。`diff` 有 200 000 字符上限（`MAX_RESULT_DIFF_CHARS`）：它的作用是就地解释改动，不是复制整份文件。
- **工具调用摘要**（`frontend/src/lib/format.ts::toolSummary`）：折叠态卡片标题由“bash”变成“bash cd /tmp/zt2 && python3 emf_report.py …”，按 `command`/`path`/`file_path`/`pattern`/`query`/`url`/`prompt` 的顺序取第一个命中的参数，压掉换行、截断 120 字符。
- **失败/截断轮次不再消失**（真实 bug）：pi 把 provider 失败写成 `content: []` + `errorMessage` 的 assistant 条目，而原解析器的“空内容就丢掉”规则会把**整轮**从转录里删掉 —— 于是“请求超时”的轮次在界面上表现为什么都没有。现在 assistant 消息带 `model` / `stop_reason` / `error_message`：`error` 显示红色 `Request timed out.` 提示、`length` 显示“输出达到长度上限，本轮回答被 Pi 截断”，标题里还多了模型名与本轮耗时（`gpt-test · 1.2s`）。同一个测试会话因此从 1415 条变成 1451 条可渲染消息（多出来的正是此前被吞掉的失败轮次）。
- **长转录分页**（`ChatPanel.tsx`）：`messages` 一次渲染上千条 markdown 会让首屏布局卡住，现在默认只渲染最后 80 条，上方一个「显示更早的 80 条（共 1451 条）」按钮按页向上展开；切会话时回到 80 条。用窗口切片而不是改 `useSession` 的数据流，所以 `SessionStats`（按整段聚合）与自动滚动（依赖消息条数与 running 状态）都不受影响。
- 测试：后端 `keeps_tool_call_parts_of_a_textless_assistant_turn` 扩展了工具名/耗时/截断/diff/模型/停止原因断言，并新增 `keeps_an_errored_assistant_turn_with_no_content`；前端 `make test` 编译清单加入 `src/lib/format.ts`，`check-frontend.mjs` 增加 `format`（9 条）/`toolSummary`（7 条）/`diffLines`（2 条）断言。
- 真机验证（headless Chrome，全部为真实 DOM 断言）：
  - 真实 Pi 会话：bundle `assets/index-CieT_Mdu.js`；首屏 79 行（窗口 80 条，含 1 条 compaction）、`显示更早的 80 条（共 1451 条）`；工具卡片标题 `bash 44ms`、`edit …`；条目元信息 `deepseek-v4.1-flash · 24s`；点开 edit 结果得到 65 行 diff 片段（`diff-add 9 / diff-remove 15 / diff-context 37 / diff-gap 4`，新增行背景 `rgb(240,250,243)`）；点「显示更早的」后行数 79 → 158 且按钮仍在；控制台无任何报错。
  - 构造的失败轮次夹具（`err1` 转录 + 一条测试会话，验完即删）：`Request timed out.` 红色提示 + `.message-row.failed`；`输出达到长度上限…` 提示；`bash 77ms 输出已截断` 与 `edit 9ms` 卡片；diff 四行类型与文本完全正确；元信息 `gpt-test · 1.2s / 900ms / 1m0s / 3.1s`（覆盖毫秒、秒、分钟三种格式）。
- 有意不做：**ANSI 上色**（922 条 toolResult 里 0 条带 ANSI，为一个不出现的情况引入 `ansi_up` 不划算）；**KaTeX/Mermaid/PDF/DOCX 预览**（体积与离线约束，见 P1.5 说明）；**逐条消息的 token/费用**（`SessionStats` 已给会话汇总，逐条展示只是噪音）；**把工具调用与工具结果合并成一张卡**（pi-web 的做法，但要重写消息分组的渲染循环，收益低于本轮三项）；**分支切换器（BranchNavigator）**（属于 pi 会话树导航，需要先设计清楚再动）。

### ✅ `make build` 的 Rust 警告清理

- `cargo build --release` / `cargo check --all-targets` 实际只剩 1 条：`src/agents/process.rs` 的 `#[cfg(unix)]` 测试模块里 `unused import: super::super::adapter::AgentConfig`（同文件 1088 行用的是完整路径）。改为直接使用导入名后，`cargo check --all-targets` 与 `make build`（release）均为 `Finished`，无任何 Rust 警告；唯一的构建提示是 Vite 的 chunk 体积建议（非 Rust）。

- 顺带修掉 `make build` 的一个失败路径：最后一步 `cp ./agentweb` 在应用**正在运行**时会因 `ETXTBSY`（文本文件忙）失败，整个 build 报错退出。改成先 cp 到 `backend/target/release/agentweb.new` 再 `mv -f` 覆盖（rename 允许替换正在运行的二进制），这样边跑边构建也能成功。

### ✅ 对话界面重排：把「思考 + 工具调用」收成一段执行过程

- 要解决的问题是观感而不是缺功能：同一窗口（80 条消息）在真实会话里是 **38 次工具调用 + 38 条工具结果 + 37 段思考**，而旧实现把每一条都渲染成独立行——工具结果各占一个头像行加一张卡片（`message-row tool-result` + `tool-result-card`），工具调用一张 `tool-call-card`，思考一个 `msg-thinking`。结果是首屏 79 行几乎全是盒子，真正的回答被埋在盒子之间；一次「看代码 → 改 → 再验证」的工具循环在视觉上被拆成十几段互不相关的块。
- **分组渲染**（`frontend/src/lib/format.ts` + `frontend/src/components/ActivitySteps.tsx`）：
  - 纯函数 `buildActivityRows` 把一“段”里的消息摊平成行：`thinking` → 思考行、`toolCall` → 工具行、工具结果按 `tool_call_id` **挂回对应的调用行**（真正的合并，不再是两张卡）、带工具调用的 assistant 里的文本 → `note` 行（过程解说）。
  - `groupTranscript` 把连续的（用户消息之后、回答之前的）工具循环合成一段 `activity`，其余消息仍是普通 `message` 条目；`isActivityMessage` 排除 `stop_reason === "error"` 的轮次，失败轮次继续按原来的红色整行显示。
  - 找不到对应调用的孤儿结果（消息窗口从中间切开时会发生）单独成行，**输出不会因为配对失败而丢**；没有 `tool_call_id` 的老转录同理。
  - `activityFacts` 汇总这段过程：工具名按出现次数合并（`思考 ×28 · edit ×2 · bash ×28 · read`）、总耗时、失败/截断/进行中步数。
  - `firstLine` 给折叠行取摘要（思考行显示推理的第一行）。
- **观感规则**：一段执行过程 = 一行摘要（`执行过程` + 事实 + `1 步出错` / `1 步输出截断` / `运行中` 徽标），默认折叠；只有**最新一段且最终回答还没出来**时才默认展开（回答一到就收起）。展开后每步是一行：图标 + 工具名 + 摘要 + 耗时（`bash wc -l a.py 已截断 77ms`），点开才看参数、输出、diff、图片。出错的步骤整行标红，且摘要显示**错误信息本身**（`edit Failed to apply edit 9ms`）而不是重复参数。历史会话因此变成“用户问题 → 执行过程（一行）→ 回答”。
- CSS：`style.css` 里删掉 `.tool-call-card` / `.tool-result-*` / `message-row.tool-result` 等旧规则，换成 `.activity-steps` / `.activity-header` / `.step-*`；`.activity-steps` 用 `width: min(100%, 900px)` + `padding-left: 39px`（border-box）对齐消息正文列，窄屏不会溢出。配色沿用消息区的原始十六进制值（该文件本来就是原始 hex 为主，`--ui-*` 变量主要在侧栏/面板区）。
- 测试：`check-frontend.mjs` 新增 `activity` 组断言（10 条）——分组边界（`message / activity / message`）、行类型与顺序、结果按 id 配对、孤儿结果不丢、`pending` 计数、同名工具合并写法、失败轮次不入组、`firstLine` 取首行。
- 真机验证（headless Chrome，真实 DOM 断言）：
  - 真实 Pi 会话（1451 条消息）：首屏从 79 行卡片变成 **3 行消息 + 2 段折叠的执行过程（各 41px）**，折叠标题 `执行过程 思考 ×28 · edit ×2 · bash ×28 · read · 1m37s 1 步出错`；整段 LCP 容器 `scrollHeight 1234`；展开后步骤行 `bash cd /tmp/zt2 && python3 emf_report.py … 44ms`、思考行摘要为其首行；`.tool-call-card` / `.tool-result-card` / `message-row.tool-result` 计数全为 0；控制台无报错。
  - 构造夹具（错误 + 截断 + diff + 未返回结果 + 过程解说，验完即删）：折叠标题 `执行过程 思考 · bash · edit · read · 86ms 1 步出错 1 步输出截断 运行中`；失败步骤 `step failed` 且摘要为 `Failed to apply edit`；`已截断` 徽标；待返回的调用显示 `运行中`；展开后 diff 四行类型/文本正确（`diff-hunk/remove/add/context`）；`note` 行渲染为正文；整行失败提示 `Request timed out.` 与 `输出达到长度上限…` 保持原样。
  - 移动端 390×844：`.activity-steps` 宽 362（`scrollWidth == clientWidth`），步骤行 321 无横向溢出，`.chat` 无横向滚动。
- 有意不做：思考/工具过程的**实时流式**渲染仍走右上角的 `AgentActivity` 浮动面板（`activity` 事件），转录只在完成后补齐——两者共用同一套视觉语言，但把实时事件也塞进转录需要重构事件流；`ANSI 上色`、`KaTeX/Mermaid` 维持前文决定；**分支切换器**仍未做。
- 顺带发现（未改）：workspace 目录不存在时，`/api/sessions/{id}/files` 返回 404 会让整个会话页显示“连接或运行异常 Not Found”，而不是只在文件面板里报错。构造夹具时踩到过一次（我删了工作目录但留了转录），与本次改动无关。

### ✅ 删除工作区目录（`DELETE /workspaces`）

- 缺口：目录选择器能「新建文件夹」（P1.7），却没有反向操作——建错的目录、废弃的工作区只能去命令行 `rm -rf`。pi-web 同样只支持创建（`app/api/cwd/browse/route.ts` 的 POST），所以这是新增能力，不是移植。
- 后端 `backend/src/api/sessions.rs::delete_workspace` + `main.rs` 的 `DELETE /workspaces`，请求体 `{"path": "..."}`：
  - `fs::canonicalize` 后必须存在、是目录、且位于 `AGENTWEB_WORKSPACE_DIR`（`canonical_home()`）之内，否则 403；根目录本身 400（`不能删除工作区根目录`）。
  - **只要还有会话指向该目录或其下级目录就拒绝**（409 `WORKSPACE_IN_USE`，`该目录下还有 N 个会话，请先删除这些会话`）：会话行是那些工作唯一的记录，删掉目录会让它们指向空路径。这条守卫也是防止一次误点删掉 `~/github` 这类“所有项目的父目录”的关键——本地实测它挡住了（6 个会话在其中）。
  - 通过后 `fs::remove_dir_all` 递归删除，返回 `{"success": true}`。错误体沿用 `{"error":{"code","message"}}`，前端 `ApiError` 直接显示 `message`。
  - 会话匹配用 `workspace = ?1 OR workspace LIKE ?2 ESCAPE '\'`：`_`/`%` 是 LIKE 通配符且 SQLite LIKE 大小写不敏感，直接拼 `path + "/%"` 会把 `/home/u/my_app` 当成 `/home/u/myXapp`。`workspace_like_prefix()` 负责转义，配单测 `escapes_like_wildcards_in_workspace_paths`。（数据库中存的是 canonical 路径——`create_session`/`create_workspace` 都已规范化——所以这里 canonical 对 canonical 比较是成立的。）
- 前端 `frontend/src/components/ChatSelectionDialog.tsx`：「新建文件夹」旁边加一个红色图标按钮（`title="删除目录 <完整路径>"`），删除的是**当前浏览的目录**；确认框写明“其中的文件会被一并永久删除，且无法恢复”。成功后调用已有的 `goUp()` 回到上一级刷新列表，并把已失效的工作目录选择清空；失败信息复用创建行下方的错误行（如 409 的友好提示）。零新增状态、零新增依赖，样式只加 `.workspace-picker-danger`。
- 接口级实测（真实 fs + 真实 DB）：删普通目录成功并消失；`$HOME` → 400；`/tmp/...` 越界 → 403；不存在 → 404；`/home/ljl/github/agentweb`（6 会话）→ 409 且目录完好；父目录 `/home/ljl/github`（会话在其下级）→ 409。
- 浏览器实测（headless Chrome，真实点击 + 接受 confirm）：选到 `/home/ljl/aw-ui-del2` → 点红色按钮 → 磁盘上目录消失、列表回到 `/home/ljl`、无错误行、控制台干净；再对 `/home/ljl/github` 点删除 → 错误行显示 `该目录下还有 6 个会话，请先删除这些会话`，目录仍在、列表未变。
- 有意不做：**删除会话时连带删工作区**（现有语义是“删会话不动文件”，`delete_session` 的确认文案也这么写）；**批量清理无会话的空目录**；**侧栏工作区分组上的删除按钮**——分组是由会话派生的，点删除必然被 `WORKSPACE_IN_USE` 挡住，除非先决定“删工作区 = 连会话一起删”，那是另一个产品决策。

### ✅ 侧栏会话管理三件套：批量删除 / 按项目删除 / 项目内新建对话（本次只重建 + 实证）

- 这三个控件**早已在源码里**（`frontend/src/components/Sidebar.tsx`，未提交的改动），但一直没被编进二进制：文件 mtime 11:31，`frontend/dist` 11:37，而当时在跑的二进制是 11:14 / 11:18 的构建产物——所以界面上根本看不到，用起来就像“功能不存在”。本次没有改任何产品代码，只做了 `make build`（13:50，内嵌包 `assets/index-BOpNgyke.js`）+ 真机实证。
  - 教训：`strings` 只打 ASCII，中文标签会被漏掉；查二进制里有没有某段界面文案要用 `rg -a`（binary-safe），例如 `rg -a -c -- session-select-toggle ./agentweb`。
- 三个入口的位置：
  - **批量删除**：侧栏「Chats」标题右侧的批量选择按钮（`title="批量选择会话"`，`ListChecks` 图标）→ 工具条 `全选` / `已选 N` / `删除`（无选中时禁用）/ 退出。删除调用 `deleteSessions(ids)`，`Promise.allSettled` 并发 `DELETE /api/sessions/{id}`，失败的 id 保留选中以便重试。
  - **按项目删除**：每个项目分组头右侧的垃圾桶（`title="删除这个项目的全部会话"`），一次删掉该分组下所有会话（置顶 / 归档两个伪分组不显示这组按钮）。
  - **项目内新建对话**：分组头右侧的 `+`（`title="在这个项目里新建会话"`），打开新建对话框时工作目录**已预填为该项目的目录**（`onNewChatInWorkspace(group.key)` → `ChatSelectionDialog`；界面上读 `.workspace-picker-footer` 的「当前工作目录 …」，输入行 `.workspace-picker-path` 只显示相对路径）。
- 真机实证（headless Chrome，真实点击 + 服务端日志）：批量删除 2 条会话 → 服务端日志两条 204 相隔 **1.6ms**（并发）；点分组垃圾桶 → 两条 204 相隔 2s；点分组 `+` → 对话框打开且 `当前工作目录` 就是该分组的工作区。删除后**工作区目录仍在**（`ls -d` 两次都能看到），与确认文案「工作区文件不会被删除。」一致——目录要删得走下面的 `DELETE /workspaces`。
- 顺带补齐了上一条 `DELETE /workspaces` 守卫的接口实证：目录下还有 2 个会话时 `409 WORKSPACE_IN_USE`（`该目录下还有 2 个会话，请先删除这些会话`），删完这 2 个会话后同一个请求返回 `200 {"success":true}` 且目录真的消失。
- 探针副作用记录（不是缺陷）：浏览器里删除是真的删了会话，第一轮探针在删除后立刻读状态读到了旧快照（显示「已选 0」「按钮 disabled」），要看服务端日志或稍后再读一次才能判断成败；夹具与测试服务器用完即清（`/tmp/aw-ui/del.db` 已清空 `del-*`，`/home/ljl/aw-del-test*` 目录已删，8099 端口已停）。

### ✅ 侧栏会话列表紧凑化（字号 ↓ / 图标 ↓ / 密度 ↑）

- 只碰尺寸，不动结构：`frontend/src/style.css` 13 处 + `frontend/src/components/Sidebar.tsx` 6 处（只改 lucide 图标的 `size`）。
- 关键数值（1400×1000，侧栏 2289 行 / 12 组）：行高 **56 → 38px**（-32%），一屏可见 **9 → 14 行**（+55%）；标题 12 → 11px，副标题 10 → 9px，行内图标 15 → 13px，行内 `…` 菜单按钮 32 → 22px，分组头按钮 22 → 20px，分组头内边距 6 → 4/5px，`Chats` 标题内边距 20/9/8 → 13/8/5px，多选框 16 → 14px，搜索框 padding 6/8 → 4/7px。
- 行高另压一刀：`.session-main { line-height: 1.25 }`（原本继承侧栏的 1.5）→ 标题行 16.5 → 13.75px、副标题行 13.5 → 11.25px。
- 坑：`.session-menu` 的尺寸**改不动**——通用规则 `.icon-button { width: 32px; height: 32px }` 出现在它之后且特异性相同（都是单类），后者胜；修法是把选择器升到 `.session .session-menu`（0,2,0），`.session-group-action` 同理写成 `.sidebar .session-group-action`。
- 量测方式：headless Chrome 里取 20 行 `getBoundingClientRect()` 中位数 + 统计一屏完整可见行数；改前基线用「往页面注入旧样式再量一次」的办法取得（56px / 9 行），因为 release 二进制在改动前就被新版覆盖了。
- 另一个容易误判的点：只跑 `cargo build` 不够——前端资产是编译期嵌进二进制的，改完 CSS 要 `npm run build`（或直接 `make build`）再重启，否则浏览器拿到的还是旧 CSS；核对办法是 `curl` 首页取 `assets/index-*.css` 再和 `frontend/dist/assets/` 对文件名。

### ✅ 设置功能补全：单弹窗设置中心 + 排版 / 对话偏好

- 缺口：设置只有「偏好」里的一个完成提示音，而 Agent/技能/插件/MCP/原生配置散在 `AgentConfigDialog` 与 `AgentCatalog` 两处，必须先在 Agent 列表里选中一个 Agent 才能看到它们。pi-web 是单个设置弹窗 + 左侧分区导航（general / models / skills / agents / plugins / mcp，后五者需要项目 cwd）。
- 合并为 `frontend/src/components/SettingsDialog.tsx`（替换并删除 `PreferencesDialog.tsx`、`AgentConfigDialog.tsx`），分区：**通用 / 技能与模板 / 插件 / MCP / 原生配置**。
  - 技能、插件、MCP、原生配置直接复用已有的 `SkillsPanel` / `PluginsPanel` / `McpPanel` 与原 `AgentConfigDialog` 的配置编辑器逻辑（原样搬成同文件的 `ConfigSection`），不复制实现。
  - 分区**首次打开才挂载**（`visited` 数组）：技能/插件/MCP 会 spawn `pi` 子进程，一打开设置就全部拉起会拖慢启动。
  - 多 Agent 时头部有 Agent 下拉（默认 Pi）；非 Pi 的 Agent 显示「当前 Agent 不支持这一项：技能、插件、MCP 由 Pi 运行时提供。」。`AgentCatalog` 的「配置」按钮现在打开这个弹窗并预选该 Agent。
- 通用分区新增（参考 pi-web 的 general，但按 agentweb 的实际情况取舍）：
  - **正文字号** 12–20px（默认 13，即 `body` 原值）、**内容宽度** 640–1600px（默认 900，即消息列原值），各有滑块、数值与「重置」。实现是 CSS 变量 `--chat-font-size` / `--chat-content-width`：字号设在 `.chat` 上，宽度替换了原先写死的 `min(100%, 900px)`（`.message-row` / `.activity-card` / `.composer` / `.composer-container` / `.activity-steps` / `.mention-picker` 六处）——不这样就只有消息列变宽、输入框和过程块还卡在 900。
  - **思考默认展开**、**引用选区**开关、**Enter / Ctrl+Enter 发送模式**（后两者是已有行为的开关化：引用选区原本常开，发送键原本固定 Enter）。
  - 保留**完成提示音**（含试听）。偏好仍是 `localStorage["agentweb.preferences"]`，不落库、换设备互不影响。
- 执行过程块（`ActivitySteps`）与思考块（`.msg-thinking`）的字号改成相对正文缩放：`.activity-steps` 基准 `calc(var(--chat-font-size) - 1.5px)`，块内固定的 11.5/11/10.5/10/12px 全部换成 `em`，这样字号 13→18 时过程块 11.5→16.5px 而默认值像素级不变（11.5/11/10.5/10）。侧栏隐藏的窄屏（≤720px）在顶栏加了一个仅移动端可见的设置入口（`.mobile-settings`），否则手机上完全进不了设置。
- 偏好解析保持“坏输入不动摇”的写法：数值型用 `Number.isFinite` 守卫 + 取整 + 夹区间，非法值退回默认；`quoteSelection` 用 `!== false` 判断，让旧存档（没有这个键）继续保持开启；`sendMode` 只认 `"ctrlEnter"`。
- 有意不移植：**深色主题**（要把 `style.css` 里约 543 处写死的浅色值换成设计变量，属独立工程）、**界面语言/i18n**（agentweb 只有中文，没有可切换的翻译）、**PWA / Web Push**（没有 service worker）、**Windows PowerShell 开关**（只在 Linux/Docker 跑）、**cache-miss 提示与 models.json 结构化编辑器**（Pi 运行时专属，此前已决定砍）、**侧栏文件位置 below|tab**（agentweb 是固定三栏）。
- 测试与真机验证：`check-frontend.mjs` 的 `preferences` 组扩到 20 条断言（各字段的合法/非法输入、区间夹取、四舍五入、旧存档 `quoteSelection` 仍为 true、`sendMode` 白名单、整个对象 deepEqual）。headless Chrome 真实点击：弹窗五个分区、`overflowX 0`；拖滑块 18px/1100px 后 `--chat-font-size`、`.chat`、`.message-content`、过程块同步、提示语切成「Ctrl / Cmd + Enter 发送」、`localStorage` 写入正确；**刷新后仍生效**（持久化）；「技能与模板」约 4s 后列出 186 个技能；「原生配置」读到 `/home/ljl/.pi/agent/models.json`（1087 字节）与已知路径列表；「重置」回到 13px/900px；宽度在 640 时消息列/输入框/过程块同时收到 640px、窄视口下被容器夹住不再溢出；移动端 390×844 下导航变横排、滑块 332px、页面与弹窗均无横向溢出；控制台无任何消息。
- 已知未修（与本次无关）：窄屏下 `.top-actions` 内容本身远超视口宽（实测超出 751px，被 `.app` 的 `overflow-x: hidden` 裁掉），这是顶栏控件在手机上过密的既有问题；新增的设置入口排在第一个所以可见。

### ✅ 设置收尾：侧栏中文化 + 技能/插件列表排版

- **中英混杂**：左下角四个按钮原本是 `Agents & runtimes` / `设置` / `Account & sessions` / `Sign out`——只有「设置」是中文。改为 `Agent 与运行时` / `设置` / `账号与会话` / `退出登录`；顺带修掉同一区域的 `No chats yet`（→`还没有会话`）、`Agent workspace`（→`Agent 工作台`），以及从该按钮打开的对话框标题 `Agents & runtimes`（→`Agent 与运行时`）、`not installed`（→`未安装`）、`No description`（→`暂无说明`）。**全站仍有其它英文文案**（对话页 `Build with your Agent`、`New chat`、`Stop agent`、文件面板 `Workspace`/`Filter files`/`No files`、终端面板 `Terminal` 等），那些不在这次的范围里，要做就一次性做 i18n。
- **技能 / 插件 / MCP 一直卡在「加载」**：`PluginsPanel` 与 `McpPanel` 的 `useEffect` 只清空状态、**没有调用 `load()`**，于是首次打开永远停在「加载」，只有手点刷新才出内容（`SkillsPanel` 是先有 `load()` 的，所以技能能出来）。两处都改成清空后 `void load()`。
- **排版混乱的根因是三层滚动条**：技能 186 条 / 提示模板 100 条 / 扩展命令 84 条，每个分组各自是一个 `max-height: 220px; overflow-y: auto` 的盒子（内部滚动高度 18281 / 9537 / 6400px），外面还套着同样在滚动的 `.settings-sections`——一次只能看见约 3 行，还要在外层和分组之间来回滚。改法：
  - 设置弹窗内的 `.skills-list` / `.mcp-list` 取消限高（`max-height: none; overflow-y: visible`），整段只保留最外层一个滚动条；分组标题 `h4` 设为粘性（`position: sticky; top: 0`），滚到哪一段都看得见当前分组名；顶部的过滤框仍是主要的定位手段（输入 `ponytail` 后只剩 12 行，分组名实时变成「技能 6 / 扩展命令 6」，无命中整组消失）。
  - 粘性标题需要 `.settings-modal` 的 `overflow` 从 `hidden` 改为 `clip`：`hidden` 会让弹窗成为一个不滚动的滚动容器，块内元素的粘性定位就失效（实测 `scrollTop` 3000 时标题跟着走，改 `clip` 后滚到 19000 时第三个分组标题稳稳钉在滚动区顶部 280px）。
  - 长名字不再挤掉同行小字：`.skills-row-head` 允许换行，`.skills-row-head code` / `.mcp-row-main code` / `.mcp-row-main > span:first-child` 加 `min-width: 0` + 省略号（技能名、`git:github.com/...` 这类长包名）。
- **设置头部元素顺序**：`X` 原本夹在说明文字和 Agent 选择器中间（`.modal-head` 是 `space-between` 的三子元素），改成 标题 / Agent 选择器 / 关闭按钮，标题 `flex: 1`，关闭按钮距右边缘 18px 与内边距对齐。
- 真机验证（headless Chrome，1400×1000 与 390×844 两档）：侧栏四个按钮文案全中文；技能段 `scrollers: []`（无嵌套滚动条）、无横向溢出、粘性标题四档滚动位均正确钉住；插件段自动加载出 `git:github.com/DietrichGebert/ponytail`、`npm:pi-memory`、`npm:billion-context`、`npm:pi-mcp-adapter` 等条目（行高 72px）且无溢出；MCP 段自动加载（本机无服务器时显示空态）；移动端 390×844 下弹窗 370px、导航横排、`overflowX` 全为 0；控制台无任何消息。
- **弹窗放大 + 技能双列**（在真机上量了之后补的）：设置弹窗从 `min(860px, 96vw) × min(660px, 100dvh - 60px)` 放大到 `min(1080px, 96vw) × min(880px, 100dvh - 48px)`，滚动区从 708×565 变成 928×785。同时把技能列表改成双列网格（`repeat(auto-fill, minmax(min(300px, 100%), 1fr))`，窄弹窗自动退回单列），说明文字最多显示两行（`-webkit-line-clamp: 2`）让卡片高度统一。
  - 量到的效果（1400×1000）：技能段总高 **32993 → 17038px**，一屏完整可见的技能行 **8 → 14 行**（约 28 个技能），卡片高度 72–88px 一致；粘性分组标题依然生效（滚到底时「扩展命令」钉在滚动区顶部）。插件/MCP 列表保持单列（只有 4 条，行高 72px）。
  - 宽度自适应实测（视口 1400 / 900 / 700 / 390 / 320）：列数 2 / 2 / 1 / 1 / 1，弹窗、滚动区、页面横向溢出全为 0（`min(300px, 100%)` 保证 320px 宽时不会因为最小列宽撑破弹窗）；移动端 390×844 导航横排不变；控制台无消息。
- 有意不做：全站英文文案的 i18n（见上）；MCP 行里的 `exposure:`、传输方式、状态值保持 Pi 的原生词汇（那是数据不是界面文案）。
