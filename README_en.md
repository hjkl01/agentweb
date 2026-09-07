# Agent Web

> A lightweight, Docker-first Web Console for AI Agents.

Agent Web is designed as a unified Web interaction layer for existing Agent runtimes such as Codex, Claude Code, OpenCode and other compatible Agents.

The project does **not** reimplement the Agent runtime. Agent capabilities, model selection, tool execution, MCP, Skills, code execution and task planning remain the responsibility of the selected Agent.

## 1. Project Positioning

Agent Web is an **Agent Web Console / Unified Agent Workspace**, not another Agent Runtime.

```text
Browser
   |
   | HTTP / WebSocket
   v
+---------------------------+
|       Agent Web           |
|                           |
| React Frontend             |
| Rust Backend               |
| Agent Adapter              |
| SQLite                     |
+-------------+-------------+
              |
              v
      Agent Runtime
       /     |      \
   Codex  Claude Code  OpenCode
      |       |          |
    Model   Model      Model
```

The key boundary is:

- **Agent Web**: UI, sessions, messages, real-time events, files/workspace display, configuration and Agent lifecycle management.
- **Agent**: reasoning, model calls, tools, shell, file editing, MCP, Skills, sub-agents and actual task execution.

## 2. Core Principles

### 2.1 Agent First

The selected Agent is the source of execution capability. Agent Web does not contain its own generic Tool Runtime or Agent Loop.

### 2.2 Model Agnostic

Agent Web does not directly call LLM providers for Agent execution. The selected Agent decides which model/provider to use.

For example:

```text
Agent Web -> Codex -> Model
Agent Web -> Claude Code -> Model
Agent Web -> OpenCode -> Model
```

### 2.3 Unified Adapter

Different Agents expose different protocols and process interfaces. Agent Web uses an `AgentAdapter` abstraction to normalize them into a common event stream.

ACP should be supported as a preferred standard integration path when available. Agent-specific adapters are used when an Agent does not expose ACP.

### 2.4 Lightweight Deployment

The default deployment is one Docker container containing the Web application and Agent runtimes.

The initial image installs **Codex only**. Other Agent runtimes are displayed in the Web UI and can be installed by the user on demand.

No Redis, PostgreSQL, Kafka, RabbitMQ, Celery or other external infrastructure is required.

## 3. Technology Stack

| Layer | Technology |
|---|---|
| Frontend | React + TypeScript + Vite |
| UI | Tailwind CSS |
| State | Zustand |
| API | REST |
| Realtime | WebSocket |
| Backend | Rust |
| Web Framework | Axum |
| Async Runtime | Tokio |
| Serialization | Serde |
| Database | SQLite |
| Database Access | SQLx |
| Code Editor | Monaco Editor |
| Terminal UI | xterm.js |
| Deployment | Docker |

The backend intentionally does **not** use Python or Node.js.

Node.js is not part of the Agent Web backend. Agent runtimes that require Node.js are installed only when the user explicitly chooses to install that Agent.

## 4. Container Architecture

The normal deployment is:

```text
+------------------------------------------------------+
|                  agentweb container                  |
|                                                      |
|  +----------------+       +----------------------+   |
|  | Rust Server    |       | Frontend static      |   |
|  | Axum/Tokio     |       | React/Vite           |   |
|  +-------+--------+       +----------------------+   |
|          |                                           |
|          +-------------------+                       |
|                              |                       |
|              +---------------+---------------+       |
|              |               |               |       |
|           Codex        Claude Code       OpenCode    |
|           installed       optional        optional   |
|                                                      |
|  /data/agentweb.db                                   |
|  /workspaces/                                        |
+------------------------------------------------------+
```

The container should mount persistent directories, for example:

```yaml
volumes:
  - ./data:/data
  - ./workspaces:/workspaces
```

### Why one container?

The target is a simple personal/self-hosted deployment. The project does not need distributed workers or service orchestration in the first version.

The Agent and Web server live in the same container so that an Agent can work directly against configured workspace paths.

## 5. Agent Installation Model

The initial Docker image should contain only the lightweight/default Agent: **Codex**.

The Web UI maintains a catalog of supported Agents:

```text
Installed
---------
✓ Codex

Available
---------
○ Claude Code       [Install]
○ OpenCode          [Install]
○ OpenClaw          [Install]
○ Other Agents      [Install]
```

When the user clicks `Install`, the Rust backend executes a controlled installation procedure for that Agent inside the same container.

After installation:

```text
Agent Catalog
     |
     v
Installation Manager
     |
     v
Agent executable / runtime
     |
     v
Agent Adapter
```

The UI should display installation progress through WebSocket events rather than blocking the browser request.

### Important design point

Installing an Agent must not turn Agent Web into a general-purpose package manager. Each supported Agent has a declarative installation definition:

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

The backend executes only definitions explicitly shipped/configured by Agent Web.

## 6. Handling Node.js-heavy Agents

A key requirement is avoiding a large default image caused by Node.js and multiple Node-based Agents.

Therefore:

1. The base image does not install Node.js merely for the Web backend.
2. Codex is installed in the base image.
3. Agents requiring Node.js are optional.
4. Installing such an Agent may install its required Node.js runtime as part of the Agent package/installation definition.
5. The Web UI clearly shows the additional runtime requirements before installation.

Example:

```text
Claude Code

Requires:
  Node.js
  Claude Code

[Install]
```

This keeps the default image small while preserving an easy one-click installation path.

## 7. Agent Adapter

Rust defines a common interface:

```rust
#[async_trait]
pub trait AgentAdapter: Send + Sync {
    async fn start(&self, config: AgentConfig) -> Result<AgentSession>;

    async fn send_message(
        &self,
        session: &mut AgentSession,
        message: String,
    ) -> Result<()>;

    async fn interrupt(
        &self,
        session: &mut AgentSession,
    ) -> Result<()>;

    async fn close(
        &self,
        session: &mut AgentSession,
    ) -> Result<()>;
}
```

Potential implementations:

```text
agents/
├── mod.rs
├── codex.rs
├── claude_code.rs
├── opencode.rs
├── openclaw.rs
└── acp.rs
```

The Adapter owns the details of starting and communicating with an Agent. The rest of the application should not depend on Agent-specific process details.

## 8. Unified Agent Events

Agent output should be normalized into a common event model.

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

Example:

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

The frontend renders these events without needing to know whether they came from Codex, Claude Code or OpenCode.

## 9. WebSocket

Realtime Agent interaction uses WebSocket:

```text
WS /api/sessions/{session_id}/events
```

Flow:

```text
Browser
  |
  | message
  v
Rust Backend
  |
  v
Agent Adapter
  |
  v
Agent
  |
  | Agent events
  v
Agent Adapter
  |
  v
Rust Backend
  |
  | WebSocket
  v
Browser
```

This allows the UI to display streaming text, tool execution, command output, file changes and completion status in real time.

## 10. Message Model

Messages should be structured instead of storing only Markdown text.

A message/event stream can contain:

```text
Text
Thinking
ToolCall
ToolResult
FileChange
Code
Image
Artifact
```

Example UI:

```text
Agent
├── Thinking
│   └── Analyzing project structure...
├── Tool Call
│   └── Read package.json
├── Tool Result
│   └── ...
├── File Change
│   └── src/main.ts
└── Text
    └── The problem has been fixed.
```

## 11. Workspace

Agent Web does not provide a replacement Tool Runtime for filesystem operations. The Agent operates on its configured working directory.

Agent Web primarily provides:

- File tree
- File preview
- Code editor
- Diff display
- Download
- Artifact display

Example:

```text
/workspaces/
└── project-a/
    ├── src/
    ├── package.json
    └── README.md
```

The workspace directory should be mounted into the container so that Agent state and project files can persist independently of the container lifecycle.

## 12. Artifact Support

Agents may create artifacts such as:

```text
.docx
.pptx
.xlsx
.pdf
.png
.zip
```

Agent Web does not generate these artifacts. It detects and presents them.

The UI can provide:

```text
Preview
Download
Open
```

## 13. SQLite

SQLite is used because the target deployment is a single-container application.

The database stores interaction-layer data only:

```text
agents
agent_configs
sessions
messages
attachments
workspaces
settings
```

The database is persisted through `/data`.

There is intentionally no PostgreSQL dependency.

## 14. Database Entities

### agents

```text
id
name
type
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

Structured Agent events can be stored separately or encoded as JSON parts where appropriate.

## 15. REST API

### Agents

```http
GET    /api/agents
POST   /api/agents
GET    /api/agents/{id}
PUT    /api/agents/{id}
DELETE /api/agents/{id}
```

### Agent Catalog / Installation

```http
GET  /api/agent-catalog
POST /api/agents/{id}/install
GET  /api/agents/{id}/status
```

### Sessions

```http
GET    /api/sessions
POST   /api/sessions
GET    /api/sessions/{id}
DELETE /api/sessions/{id}
POST   /api/sessions/{id}/interrupt
```

### Messages

```http
POST /api/sessions/{id}/messages
```

### Workspace

```http
GET /api/sessions/{id}/files
GET /api/sessions/{id}/files/{path}
GET /api/sessions/{id}/diff
```

### Realtime

```text
WS /api/sessions/{id}/events
```

## 16. Frontend Layout

The primary interface follows the AionUi-style workspace concept:

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
│              │ File changes             │               │
│              │                          │ Diff          │
├──────────────┴──────────────────────────┴───────────────┤
│ Message input                                  Send    │
└─────────────────────────────────────────────────────────┘
```

## 17. Agent Settings

The Agent management page should show both installed and available Agents.

Example:

```text
Agents

Codex
Status: Installed
Version: ...
Workspace: /workspaces/project

Claude Code
Status: Not Installed
Requires: Node.js
[Install]

OpenCode
Status: Not Installed
Requires: Node.js
[Install]
```

After installation the user can configure:

```text
Name
Command
Working Directory
Environment Variables
Default Arguments
Enabled
```

## 18. Session Lifecycle

```text
Created
  |
  v
Starting
  |
  v
Running
  |
  +----> Waiting
  |         |
  |         v
  +------ Running
  |
  v
Completed
```

The backend should retain enough process/session state to reconnect the Web UI without forcing a new Agent session whenever the browser refreshes.

## 19. Project Structure

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
│   │   ├── api/
│   │   ├── websocket/
│   │   ├── agents/
│   │   ├── sessions/
│   │   ├── workspace/
│   │   ├── database/
│   │   ├── events/
│   │   ├── installation/
│   │   └── config/
│   ├── migrations/
│   └── Cargo.toml
│
├── docker/
│   └── ...
│
├── docker-compose.yml
└── README.md
```

## 20. MVP

The first version should remain deliberately small.

### Agent management

- Codex preinstalled
- Agent catalog
- Install optional Agents
- Agent configuration
- Agent status/version detection

### Session

- Create session
- Select Agent
- Select workspace
- Session list
- Rename/delete session
- Restore session

### Chat

- Send messages
- Streaming output
- Stop Agent
- WebSocket reconnect

### Events

- Text
- Thinking
- Tool Call
- Tool Result
- File Change
- Error
- Completed

### Workspace

- File tree
- File preview
- Code editor
- Diff
- Download

### Storage

- SQLite

### Deployment

- Docker
- Persistent `/data`
- Persistent `/workspaces`

## 21. Explicit Non-Goals

The following are intentionally outside the Agent Web core:

```text
Agent Loop
LLM Provider SDK integration for Agent execution
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

If an Agent provides these capabilities, Agent Web exposes their events and results rather than implementing them again.

## 22. Roadmap

### V0.1

```text
Rust server
React UI
SQLite
Codex
Agent Adapter
WebSocket
Session management
Workspace display
Docker
```

### V0.2

```text
ACP
Agent catalog
One-click Agent installation
Claude Code
OpenCode
Improved file preview
Diff viewer
Terminal/event viewer
```

### V0.3

```text
More Agent adapters
Remote Agent connections
Artifact preview
Multiple workspaces
Authentication
Permission controls
```

### V1.0

```text
Unified Agent Web Console
```

## 23. Design Summary

The final architecture is intentionally simple:

```text
                     Agent Web
                         |
             +-----------+-----------+
             |                       |
          React UI              Rust Backend
                                     |
                         +-----------+-----------+
                         |           |           |
                       SQLite     Adapter     Installer
                                     |
                 +-------------------+-------------------+
                 |                   |                   |
               Codex          Claude Code           OpenCode
                 |                   |                   |
               Model               Model               Model
```

The most important architectural rule is:

> **Rust is the unified Web gateway; the selected Agent is the actual brain and execution engine.**

This keeps Agent Web lightweight, avoids duplicating Agent functionality, minimizes infrastructure dependencies, and makes adding new Agents primarily an Adapter + installation definition problem.
