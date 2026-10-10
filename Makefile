.DEFAULT_GOAL := help

SHELL := /bin/sh

APP_NAME ?= agentweb
IMAGE ?= $(APP_NAME):latest
COMPOSE ?= docker compose
BUILD_DIR ?= build
DATA_DIR ?= data
DEBUG_BINARY := backend/target/debug/agentweb
RELEASE_BINARY := backend/target/release/agentweb

.PHONY: help install dev dev-backend dev-frontend build release start check test fmt fmt-frontend lint clean docker-build docker-up docker-down docker-restart docker-logs docker-shell docker-pull docker-clean

help: ## Show available commands
	@echo "Agent Web"
	@echo ""
	@echo "Usage: make <command>"
	@echo ""
	@echo "Development:"
	@grep -E '^(install|dev|dev-backend|dev-frontend|build|release|start|check|test|fmt|fmt-frontend|lint|clean):.*##' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*## "}; {printf "  %-18s %s\n", $$1, $$2}'
	@echo ""
	@echo "Docker:"
	@grep -E '^docker-(build|up|down|restart|logs|shell|pull|clean):.*##' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*## "}; {printf "  %-18s %s\n", $$1, $$2}'

install: ## Install frontend dependencies and fetch Rust dependencies
	@mkdir -p $(DATA_DIR)/home $(DATA_DIR)/workspaces $(DATA_DIR)/runtimes/node $(DATA_DIR)/runtimes/agents
	cd frontend && npm install --no-audit --no-fund
	cd backend && cargo fetch

dev: ## Start backend and frontend development servers
	@trap 'kill 0' INT TERM EXIT; \
		(cd frontend && npm run dev -- --host 127.0.0.1) & \
		cd backend && cargo run

dev-backend: ## Start Rust backend development server
	@mkdir -p $(DATA_DIR)/home $(DATA_DIR)/workspaces $(DATA_DIR)/runtimes/node $(DATA_DIR)/runtimes/agents
	cd backend && cargo run

dev-frontend: ## Start Vite frontend development server
	cd frontend && npm run dev -- --host 127.0.0.1

build: ## Build frontend and backend into ./build for direct startup
	cd frontend && npm run build
	cd backend && cargo build
	@mkdir -p $(BUILD_DIR)
	@rm -rf $(BUILD_DIR)/frontend
	@cp $(DEBUG_BINARY) $(BUILD_DIR)/agentweb
	@cp -R frontend/dist $(BUILD_DIR)/frontend
	@echo "构建完成：$(BUILD_DIR)/agentweb 和 $(BUILD_DIR)/frontend"

release: ## Build optimized frontend/backend package into ./build
	cd frontend && npm run build
	cd backend && cargo build --release
	@mkdir -p $(BUILD_DIR)
	@rm -rf $(BUILD_DIR)/frontend
	@cp $(RELEASE_BINARY) $(BUILD_DIR)/agentweb
	@cp -R frontend/dist $(BUILD_DIR)/frontend
	@echo "构建完成：$(BUILD_DIR)/agentweb 和 $(BUILD_DIR)/frontend"

start: ## Start the packaged app from ./build
	@test -x $(BUILD_DIR)/agentweb || { echo "未找到 $(BUILD_DIR)/agentweb，请先运行 make build 或 make release" >&2; exit 1; }
	@test -f $(BUILD_DIR)/frontend/index.html || { echo "未找到前端文件，请先运行 make build 或 make release" >&2; exit 1; }
	@mkdir -p $(DATA_DIR)/home $(DATA_DIR)/workspaces $(DATA_DIR)/runtimes/node $(DATA_DIR)/runtimes/agents
	HOME="$(CURDIR)/$(DATA_DIR)/home" \
	XDG_CONFIG_HOME="$(CURDIR)/$(DATA_DIR)/home/.config" \
	XDG_DATA_HOME="$(CURDIR)/$(DATA_DIR)/home/.local/share" \
	XDG_CACHE_HOME="$(CURDIR)/$(DATA_DIR)/home/.cache" \
	DATABASE_URL="sqlite://$(CURDIR)/$(DATA_DIR)/agentweb.db" \
	AGENTWEB_WORKSPACE_DIR="$(CURDIR)/$(DATA_DIR)/workspaces" \
	AGENTWEB_RUNTIME_DIR="$(CURDIR)/$(DATA_DIR)/runtimes/node" \
	AGENTWEB_FRONTEND_DIR="$(CURDIR)/$(BUILD_DIR)/frontend" \
	./$(BUILD_DIR)/agentweb

check: ## Run Rust checks and frontend build checks
	cd backend && cargo check
	cd frontend && npm run build

test: ## Run Rust tests
	cd backend && cargo test

fmt: ## Format Rust and frontend code
	cd backend && cargo fmt
	$(MAKE) fmt-frontend

fmt-frontend: ## Format frontend code with Prettier
	npx --yes prettier@3 --write frontend/src frontend/*.json frontend/*.html

lint: ## Run Rust Clippy with warnings treated as errors
	cd backend && cargo clippy --all-targets --all-features -- -D warnings

clean: ## Remove local build artifacts
	cd backend && cargo clean
	rm -rf frontend/dist $(BUILD_DIR)

# -----------------------------------------------------------------------------
# Docker
# -----------------------------------------------------------------------------

docker-build: ## Build the Docker image
	$(COMPOSE) build
docker-up: ## Build and start Docker services
	$(COMPOSE) up -d
docker-down: ## Stop Docker services
	$(COMPOSE) down
docker-restart: ## Restart Docker services
	$(COMPOSE) restart
docker-logs: ## Follow Agent Web container logs
	$(COMPOSE) logs -f agentweb
docker-shell: ## Open a shell inside the Agent Web container
	$(COMPOSE) exec agentweb /bin/bash
docker-pull: ## Pull Docker base images
	docker pull node:22-bookworm
	docker pull rust:1.88-bookworm
	docker pull archlinux:base
docker-clean: ## Stop services and remove local Docker images
	$(COMPOSE) down --rmi local --remove-orphans
