.DEFAULT_GOAL := help

SHELL := /bin/sh

APP_NAME ?= agentweb
IMAGE ?= $(APP_NAME):latest
COMPOSE ?= docker compose
DATA_DIR ?= data
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
	cd frontend && npm install --no-audit --no-fund
	cd backend && cargo fetch

dev: ## Start backend and frontend development servers
	@trap 'kill 0' INT TERM EXIT; \
		(cd frontend && npm run dev -- --host 127.0.0.1) & \
		cd backend && cargo run

dev-backend: ## Start Rust backend development server
	cd backend && cargo run

dev-frontend: ## Start Vite frontend development server
	cd frontend && npm run dev -- --host 127.0.0.1

build: install ## Build frontend and backend into a standalone binary in the project root
	cd frontend && npm run build
	cd backend && cargo build --release
	@cp $(RELEASE_BINARY) $(RELEASE_BINARY).new && mv -f $(RELEASE_BINARY).new ./agentweb	# 直接覆盖运行中的二进制会 ETXTBSY
	@echo "构建完成：./agentweb（前端已嵌入二进制）"

release: build ## Build the optimized standalone binary (alias of build)

start: ## Start the packaged app from ./agentweb
	@test -x ./agentweb || { echo "未找到 ./agentweb，请先运行 make build 或 make release" >&2; exit 1; }
	./agentweb

check: ## Run Rust checks and frontend build checks
	cd backend && cargo check
	cd frontend && npm run build
	./frontend/scripts/check-names.sh

test: ## Run Rust tests and frontend unit checks
	cd backend && cargo test
	cd frontend && rm -rf .tmp-test && ./node_modules/.bin/tsc src/lib/format.ts src/lib/sessionFamily.ts src/lib/preferences.ts src/lib/sessionGroups.ts src/lib/pendingMessages.ts --outDir .tmp-test --module esnext --target esnext --moduleResolution bundler && node scripts/check-frontend.mjs; status=$$?; rm -rf .tmp-test; exit $$status

fmt: ## Format Rust and frontend code
	cd backend && cargo fmt
	$(MAKE) fmt-frontend

fmt-frontend: ## Format frontend code with Prettier
	npx --yes prettier@3 --write frontend/src frontend/*.json frontend/*.html

lint: ## Run Rust Clippy with warnings treated as errors
	cd backend && cargo clippy --all-targets --all-features -- -D warnings

clean: ## Remove local build artifacts
	cd backend && cargo clean
	rm -rf frontend/dist

# -----------------------------------------------------------------------------
# Docker
# -----------------------------------------------------------------------------

docker-build: ## Build the Docker image
	$(COMPOSE) build
docker-up: ## Build and start Docker services
	@mkdir -p "${AGENTWEB_RUNTIME_DIR:-$HOME/.local/share/agentweb/runtimes}"
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
