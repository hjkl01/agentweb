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

FROM archlinux:base
RUN sed -i 's/^#\?Server = .*/Server = https:\/\/mirrors.aliyun.com\/archlinux\/$repo\/os\/$arch/' /etc/pacman.d/mirrorlist \
  && pacman -Syu --noconfirm \
    ca-certificates \
    curl \
    xz \
    tar \
    git \
    procps-ng \
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
    nginx \
  && pacman -Scc --noconfirm
COPY --from=backend /src/backend/target/release/agentweb /usr/local/bin/agentweb
COPY --from=frontend /src/frontend/dist /app/frontend
COPY docker/nginx.conf /etc/agentweb/nginx.conf.template
COPY docker/entrypoint.sh /usr/local/bin/agentweb-entrypoint
RUN chmod +x /usr/local/bin/agentweb-entrypoint \
  && mkdir -p /data/home /workspaces /data/runtimes/node /data/runtimes/agents /etc/agentweb
ENV HOME=/data/home
ENV XDG_CONFIG_HOME=/data/home/.config
ENV XDG_DATA_HOME=/data/home/.local/share
ENV XDG_CACHE_HOME=/data/home/.cache
ENV DATABASE_URL=sqlite:///data/agentweb.db
ENV AGENTWEB_WORKSPACE_DIR=/workspaces
ENV AGENTWEB_RUNTIME_DIR=/data/runtimes/node
ENV AGENTWEB_FRONTEND_DIR=/app/frontend
ENV AGENTWEB_WORKERS=1
ENV RUST_LOG=info
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/agentweb-entrypoint"]
