.DEFAULT_GOAL := help

SHELL := /bin/sh

APP_NAME ?= agentweb
IMAGE ?= $(APP_NAME):latest
COMPOSE ?= docker compose

.PHONY: help install dev dev-backend dev-frontend build release check test fmt lint clean docker-build docker-up docker-down docker-restart docker-logs docker-shell docker-pull docker-clean

help: ## Show available commands
	@echo "Agent Web"
	@echo ""
	@echo "Usage: make <command>"
	@echo ""
	@echo "Development:"
	@grep -E '^(install|dev|dev-backend|dev-frontend|build|release|check|test|fmt|lint|clean):.*##' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*## "}; {printf "  %-18s %s\n", $$1, $$2}'
	@echo ""
	@echo "Docker:"
	@grep -E '^docker-(build|up|down|restart|logs|shell|pull|clean):.*##' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*## "}; {printf "  %-18s %s\n", $$1, $$2}'

install: ## Install frontend dependencies and fetch Rust dependencies
	@mkdir -p data workspaces runtimes
	cd frontend && npm install --no-audit --no-fund
	cd backend && cargo fetch

dev: ## Start backend and frontend development servers
	@trap 'kill 0' INT TERM EXIT; \
		(cd frontend && npm run dev -- --host 127.0.0.1) & \
		cd backend && cargo run

dev-backend: ## Start Rust backend development server
	@mkdir -p data workspaces runtimes
	cd backend && cargo run

dev-frontend: ## Start Vite frontend development server
	cd frontend && npm run dev -- --host 127.0.0.1

build: ## Build frontend and Rust backend in debug mode
	cd frontend && npm run build
	cd backend && cargo build

release: ## Build frontend and optimized Rust backend
	cd frontend && npm run build
	cd backend && cargo build --release

check: ## Run Rust checks and frontend build checks
	cd backend && cargo check
	cd frontend && npm run build

test: ## Run Rust tests
	cd backend && cargo test

fmt: ## Format Rust code
	cd backend && cargo fmt

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
	$(COMPOSE) up -d --build

docker-down: ## Stop Docker services
	$(COMPOSE) down

docker-restart: ## Restart Docker services
	$(COMPOSE) restart

docker-logs: ## Follow Agent Web container logs
	$(COMPOSE) logs -f agentweb
docker-shell: ## Open a shell inside the Agent Web container
	$(COMPOSE) exec agentweb /bin/sh

docker-pull: ## Pull Docker base images
	docker pull node:22-bookworm
	docker pull rust:1.88-bookworm
	docker pull debian:bookworm-slim

docker-clean: ## Stop services and remove local Docker images
	$(COMPOSE) down --rmi local --remove-orphans
