# Agent Web 技术文档

- [architecture.md](./architecture.md)：整体技术方案、Agent Adapter、模型、Session、Workspace、认证与部署架构。
- [nginx.conf](./nginx.conf)：Nginx HTTPS 反向代理示例，包含 WebSocket 转发。
- [Caddyfile](./Caddyfile)：Caddy 反向代理示例，包含自动 HTTPS 和 WebSocket。

## 生产部署

推荐：

```text
Internet
   ↓ HTTPS
Nginx / Caddy
   ↓ HTTP :8080
Agent Web
   ├── SQLite /data
   ├── Workspaces /workspaces
   └── Agent runtimes /runtimes
```

不要直接把 8080 暴露到公网；生产环境使用 HTTPS，并确保反向代理正确传递客户端 IP Header。登录保护策略为连续 3 次失败后 IP 锁定 30 分钟。

Swagger：`http://localhost:8080/docs`。
