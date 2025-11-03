.PHONY: help test test-unit test-integration test-all coverage build clean clippy fmt check

help: ## Show this help message
	@echo 'Usage: make [target]'
	@echo ''
	@echo 'Available targets:'
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2}'

test-unit: ## Run unit tests only
	cargo test --lib --bins

test-integration: ## Run integration tests only
	cargo test --test '*'

test-doc: ## Run documentation tests
	cargo test --doc

test: test-unit test-integration test-doc ## Run all tests

test-all: test ## Alias for test

coverage: ## Generate code coverage report
	cargo tarpaulin --verbose --all-features --workspace --timeout 120 --out html
	@echo "Coverage report generated in tarpaulin-report.html"

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
	cargo install cargo-tarpaulin cargo-watch
