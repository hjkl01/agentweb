.DEFAULT_GOAL := help

SHELL := /bin/sh

APP_NAME ?= agentweb
IMAGE ?= $(APP_NAME):latest
COMPOSE ?= docker compose

.PHONY: help dev frontend backend build release check test fmt lint clean \
        docker-build docker-up docker-down docker-restart docker-logs docker-shell \
        docker-pull docker-clean install

help: ## Show available commands
	@echo "Agent Web"
	@echo ""
	@echo "Local development:"
	@grep -E '^(dev|frontend|backend|build|release|check|test|fmt|lint|clean|install):.*##' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*## "}; {printf "  %-18s %s\n", $$1, $$2}'
	@echo ""
	@echo "Docker:"
	@grep -E '^(docker-build|docker-up|docker-down|docker-restart|docker-logs|docker-shell|docker-pull|docker-clean):.*##' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*## "}; {printf "  %-18s %s\n", $$1, $$2}'

install: ## Install frontend dependencies
	cd frontend && npm install

dev: ## Start frontend development server
	cd frontend && npm run dev

frontend: ## Build frontend
	cd frontend && npm run build

backend: ## Build Rust backend in debug mode
	cd backend && cargo build

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
