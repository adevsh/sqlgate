.PHONY: help setup install dev fmt lint test build clean tailwind assets

TAILWIND_VERSION := v4.1.18
TAILWIND_BIN := ./bin/tailwindcss
ALPINE_VERSION := 3.14.9

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

assets: static/alpine.min.js ## Download vendor static assets

static/alpine.min.js:
	@mkdir -p static
	curl -sSLo static/alpine.min.js \
		https://cdn.jsdelivr.net/npm/alpinejs@$(ALPINE_VERSION)/dist/cdn.min.js
	@echo "Alpine.js $(ALPINE_VERSION) vendored to static/"

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

build: tailwind ## Build release binary + Tailwind CSS
	$(TAILWIND_BIN) -i static/tailwind.input.css -o static/tailwind.css --minify
	cargo build --release

clean: ## Remove build artifacts
	cargo clean
	rm -f static/tailwind.css
