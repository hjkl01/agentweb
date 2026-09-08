FROM node:22-bookworm AS frontend
WORKDIR /src/frontend
COPY frontend/package.json ./
RUN npm config set registry https://registry.npmmirror.com \
  && npm install --no-audit --no-fund
COPY frontend ./
RUN npm run build

FROM rust:1.88-bookworm AS backend
WORKDIR /src
COPY backend/Cargo.toml backend/Cargo.lock ./backend/
COPY backend ./backend
ENV CARGO_REGISTRIES_CRATES_IO_INDEX=sparse+https://mirrors.cernet.edu.cn/crates.io-index/
RUN cd backend && cargo build --release

FROM debian:bookworm-slim
RUN sed -i \
  -e 's@deb.debian.org@mirrors.aliyun.com@g' \
  -e 's@security.debian.org@mirrors.aliyun.com@g' \
  /etc/apt/sources.list.d/debian.sources 2>/dev/null || true \
  && apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates curl git procps util-linux xz-utils \
  && rm -rf /var/lib/apt/lists/*
COPY --from=backend /src/backend/target/release/agentweb /usr/local/bin/agentweb
COPY --from=frontend /src/frontend/dist /app/frontend
RUN mkdir -p /data/home /workspaces /opt/agent-runtimes/node /opt/agent-runtimes/agents
ENV HOME=/data/home
ENV XDG_CONFIG_HOME=/data/home/.config
ENV XDG_DATA_HOME=/data/home/.local/share
ENV XDG_CACHE_HOME=/data/home/.cache
ENV DATABASE_URL=sqlite:///data/agentweb.db
ENV AGENTWEB_WORKSPACE_DIR=/workspaces
ENV AGENTWEB_RUNTIME_DIR=/opt/agent-runtimes/node
ENV AGENTWEB_FRONTEND_DIR=/app/frontend
ENV RUST_LOG=info
EXPOSE 8080
CMD ["/usr/local/bin/agentweb"]
