.PHONY: help setup install dev fmt lint test db-migrate build clean tailwind assets

TAILWIND_VERSION := v4.1.18
TAILWIND_BIN := ./bin/tailwindcss
HTMX_VERSION := 4.0.0-beta.5

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*## ' $(MAKEFILE_LIST) \
		| sort \
		| awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-12s\033[0m %s\n", $$1, $$2}'

setup: ## Install toolchain prerequisites + download Tailwind + Alpine
	@rustup component add clippy rustfmt 2>/dev/null; true
	@$(MAKE) tailwind assets

tailwind: $(TAILWIND_BIN) ## Download Tailwind standalone CLI (no npm required)

$(TAILWIND_BIN):
	@mkdir -p bin
	curl -sSLo $(TAILWIND_BIN) \
		https://github.com/tailwindlabs/tailwindcss/releases/download/$(TAILWIND_VERSION)/tailwindcss-macos-arm64
	chmod +x $(TAILWIND_BIN)
	@echo "tailwindcss $(TAILWIND_VERSION) installed to bin/"

assets: static/alpine.min.js static/htmx.min.js ## Download vendor static assets

static/alpine.min.js:
	@mkdir -p static
	curl -sSLo static/alpine.min.js \
		https://cdn.jsdelivr.net/npm/alpinejs@$(ALPINE_VERSION)/dist/cdn.min.js
	@echo "Alpine.js $(ALPINE_VERSION) vendored to static/"

static/htmx.min.js:
	@mkdir -p static
	curl -sSLo static/htmx.min.js \
		https://github.com/bigskysoftware/htmx/releases/download/v$(HTMX_VERSION)/htmx.min.js
	@echo "HTMX $(HTMX_VERSION) vendored to static/"

install: ## Build release binary
	cargo build --release

dev: ## Build debug binary
	cargo build

fmt: ## Format code
	cargo fmt

lint: ## Run clippy
	cargo clippy -- -D warnings

test: ## Run all tests
	cargo test

db-up: ## Start all databases via docker compose
	docker compose up -d --wait

db-down: ## Stop and remove databases (keeps volumes)
	docker compose down

db-reset: ## Stop, remove volumes, restart from scratch
	docker compose down -v
	docker compose up -d --wait

db-migrate: ## Apply schema.sql to Postgres (plain psql -f, no migration framework)
build: tailwind ## Build release binary + Tailwind CSS
	$(TAILWIND_BIN) -i static/tailwind.input.css -o static/tailwind.css --minify
	cargo build --release
clean: ## Remove build artifacts
	cargo clean
	rm -f static/tailwind.css
