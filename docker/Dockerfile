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
COPY --from=frontend /src/frontend/dist /src/frontend/dist
ENV CARGO_REGISTRIES_CRATES_IO_INDEX=sparse+https://mirrors.cernet.edu.cn/crates.io-index/
RUN cd backend && cargo build --release

FROM archlinux:base
# Keep the Arch base image's mirrorlist instead of rewriting every mirror
# entry to Aliyun; duplicate entries can all point at a stale mirror and cause 404s.
RUN pacman -Syu --noconfirm \
    ca-certificates \
    tzdata \
    curl \
    xz \
    tar \
    git \
    procps-ng \
    bubblewrap \
    util-linux \
    bash \
    coreutils \
    grep \
    sed \
    gawk \
    findutils \
    openssh \
    ripgrep \
    fd \
    unzip \
    which \
    base-devel \
    python \
    sqlite \
    nginx \
  && ln -sf /usr/share/zoneinfo/Asia/Shanghai /etc/localtime \
  && printf 'Asia/Shanghai\n' > /etc/timezone \
  && pacman -Scc --noconfirm
COPY --from=backend /src/backend/target/release/agentweb /usr/local/bin/agentweb
COPY docker/nginx.conf /etc/agentweb/nginx.conf.template
COPY docker/entrypoint.sh /usr/local/bin/agentweb-entrypoint
RUN chmod +x /usr/local/bin/agentweb-entrypoint \
  && mkdir -p /home/agent /workspaces /agent-runtimes /data /etc/agentweb
ENV TZ=Asia/Shanghai
ENV HOME=/home/agent
ENV AGENTWEB_AGENT_HOME=/home/agent
ENV XDG_CONFIG_HOME=/home/agent/.config
ENV XDG_DATA_HOME=/home/agent/.local/share
ENV XDG_CACHE_HOME=/home/agent/.cache
ENV DATABASE_URL=sqlite:///home/agent/.config/agentweb/agentweb.db
ENV AGENTWEB_WORKSPACE_DIR=/workspaces
ENV AGENTWEB_RUNTIME_DIR=/agent-runtimes
ENV AGENTWEB_WORKERS=1
ENV RUST_LOG=info
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/agentweb-entrypoint"]
