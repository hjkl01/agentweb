.DEFAULT_GOAL := help

SHELL := /bin/sh

APP_NAME ?= agentweb
IMAGE ?= $(APP_NAME):latest
COMPOSE ?= docker compose

.PHONY: help install install-frontend install-backend \
        dev frontend frontend-install backend backend-install backend-run backend-release backend-release-run \
        build release check test fmt lint clean \
        docker-build docker-up docker-down docker-restart docker-logs docker-shell \
        docker-pull docker-clean up down logs shell

help: ## Show available commands
	@echo "Agent Web"
	@echo ""
	@echo "Local development:"
	@grep -E '^(install|install-frontend|install-backend|dev|frontend|frontend-install|backend|backend-install|backend-run|backend-release|backend-release-run|build|release|check|test|fmt|lint|clean):.*##' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*## "}; {printf "  %-22s %s\n", $$1, $$2}'
	@echo ""
	@echo "Docker:"
	@grep -E '^(docker-build|docker-up|docker-down|docker-restart|docker-logs|docker-shell|docker-pull|docker-clean):.*##' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*## "}; {printf "  %-22s %s\n", $$1, $$2}'

install: install-frontend install-backend ## Install all local development dependencies

install-frontend: ## Install frontend npm dependencies
	cd frontend && npm install

install-backend: ## Download Rust backend dependencies
	cd backend && cargo fetch

dev: ## Start frontend development server
	cd frontend && npm run dev

frontend: ## Build frontend
	cd frontend && npm run build

frontend-install: install-frontend

backend: ## Build Rust backend in debug mode
	cd backend && cargo build

backend-install: install-backend

backend-run: ## Start Rust backend in development mode
	cd backend && cargo run

backend-release: ## Build optimized Rust backend
	cd backend && cargo build --release

backend-release-run: ## Start optimized Rust backend
	cd backend && cargo run --release

build: ## Build frontend and Rust backend
	$(MAKE) frontend
	$(MAKE) backend

release: ## Build optimized Rust backend and frontend
	cd frontend && npm run build
	cd backend && cargo build --release

check: ## Run Rust checks and frontend type/build checks
	cd backend && cargo check
	cd frontend && npm run build

test: ## Run Rust tests
	cd backend && cargo test

fmt: ## Format Rust code
	cd backend && cargo fmt

lint: ## Run Rust clippy
	cd backend && cargo clippy --all-targets --all-features -- -D warnings

clean: ## Clean local build artifacts
	cd backend && cargo clean
	rm -rf frontend/dist

docker-build: ## Build Docker image
	$(COMPOSE) build

docker-up: ## Build and start Docker services
	$(COMPOSE) up -d

docker-down: ## Stop Docker services
	$(COMPOSE) down

docker-restart: ## Restart Docker services
	$(COMPOSE) restart

docker-logs: ## Follow Docker service logs
	$(COMPOSE) logs -f agentweb

docker-shell: ## Open a shell inside the Agent Web container
	$(COMPOSE) exec agentweb /bin/sh

docker-pull: ## Pull Docker base images before building
	docker pull node:22-bookworm
	docker pull rust:1.88-bookworm
	docker pull debian:bookworm-slim

docker-clean: ## Remove the Agent Web container and image
	$(COMPOSE) down --rmi local --remove-orphans

# Optional aliases for users who prefer shorter commands.
up: docker-up
down: docker-down
logs: docker-logs
shell: docker-shell
