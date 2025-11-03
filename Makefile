.PHONY: help test test-unit test-integration test-all coverage build clean clippy fmt check

help: ## Show this help message
	@echo 'Usage: make [target]'
	@echo ''
	@echo 'Available targets:'
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2}'

test-unit: ## Run unit tests only
	cargo test --bins

test-integration: ## Run integration tests only
	cargo test --test '*'

test-doc: ## Run documentation tests (N/A for binary-only crates)
	@echo "No doc tests available for binary-only crate"

test: test-unit test-integration ## Run all tests

test-all: test ## Alias for test

coverage: ## Generate code coverage report
	cargo llvm-cov --html
	@echo "Coverage report generated in target/llvm-cov/html/index.html"

coverage-text: ## Show coverage summary in terminal
	cargo llvm-cov

build: ## Build the project in debug mode
	cargo build

build-release: ## Build the project in release mode
	cargo build --release

clean: ## Clean build artifacts
	cargo clean

clippy: ## Run clippy linter
	cargo clippy --all-targets --all-features -- -D warnings

fmt: ## Format code
	cargo fmt

fmt-check: ## Check code formatting
	cargo fmt -- --check

check: fmt-check clippy test ## Run all checks (fmt, clippy, tests)

run: ## Run the proxy in debug mode
	RUST_LOG=debug cargo run

watch: ## Watch for changes and run tests
	cargo watch -x test

install-tools: ## Install development tools
	rustup component add llvm-tools-preview
	cargo install cargo-llvm-cov cargo-watch
