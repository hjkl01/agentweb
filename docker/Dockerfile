FROM node:22-bookworm AS frontend
WORKDIR /src/frontend
COPY frontend/package.json ./
RUN npm install
COPY frontend ./
RUN npm run build

FROM rust:1.88-bookworm AS backend
WORKDIR /src
COPY backend/Cargo.toml ./backend/
COPY backend ./backend
RUN cd backend && cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl git procps xz-utils && rm -rf /var/lib/apt/lists/*
COPY --from=backend /src/backend/target/release/agentweb /usr/local/bin/agentweb
COPY --from=frontend /src/frontend/dist /app/frontend
RUN mkdir -p /data/home /workspaces /opt/agent-runtimes/node /opt/agent-runtimes/agents
ENV HOME=/data/home
ENV XDG_CONFIG_HOME=/data/home/.config
ENV XDG_DATA_HOME=/data/home/.local/share
ENV XDG_CACHE_HOME=/data/home/.cache
ENV DATABASE_URL=sqlite:///data/agentweb.db
ENV RUST_LOG=info
EXPOSE 8080
CMD ["/usr/local/bin/agentweb"]
