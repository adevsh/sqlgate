.PHONY: help setup install dev fmt lint test build clean

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*## ' $(MAKEFILE_LIST) \
		| sort \
		| awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-12s\033[0m %s\n", $$1, $$2}'

setup: ## Install toolchain prerequisites
	@rustup component add clippy rustfmt 2>/dev/null; true

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

build: ## Build release binary (alias for install)
	cargo build --release

clean: ## Remove build artifacts
	cargo clean
