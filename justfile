# https://just.systems
# Converted from Taskfile.yml

# Variables
binary_name := "bun-docs-mcp-proxy"
build_dir := "target/release"
coverage_dir := "target/llvm-cov/html"
doc_dir := "target/doc"

# Dynamic variables (evaluated at runtime)
current_version := `cargo pkgid | cut -d# -f2`
commit_hash := `git rev-parse HEAD`
build_time := `date -u +"%Y-%m-%dT%H:%M:%SZ"`

# Environment
export CARGO_TERM_COLOR := "always"

# ===== Aliases =====
alias br := build-release
alias release := build-release
alias t := test
alias tu := test-unit
alias tio := test-integration-only
alias twi := test-with-integration
alias ti := test-integration
alias tn := test-nextest
alias cov := coverage
alias covh := coverage-html
alias covt := coverage-text
alias fc := fmt-check
alias lint := clippy
alias lint-strict := clippy-strict
alias c := check
alias rr := run-release
alias s := search
alias sr := search-release
alias d := dev
alias dpo := doc-pages-open

# Default recipe - show available recipes
default:
    @just --list

# ===== Build Tasks =====

# Build debug binary with all features and targets
[group('build')]
build *args:
    cargo build --all-features --all-targets {{ args }}

# Build optimized release binary
[group('build')]
build-release:
    cargo build --release
    @echo "Built {{ binary_name }} version {{ current_version }} (commit {{ commit_hash }}) at {{ build_time }}"
    @echo "Binary: {{ build_dir }}/{{ binary_name }}"

# Remove build artifacts
[group('build')]
[confirm("Delete all build artifacts?")]
clean:
    cargo clean

# Update dependencies in Cargo.lock
[group('build')]
update *args:
    cargo update {{ args }}

# ===== Test Tasks =====

# Run all tests (unit tests only by default, no network calls)
[group('test')]
test *args:
    cargo test {{ args }}

# Run unit tests only (fast, no network)
[group('test')]
test-unit *args:
    cargo test --bins {{ args }}

# Run integration tests with real API calls (requires network)
[group('test')]
test-integration-only *args:
    cargo test --features integration-tests {{ args }}

# Run all tests including integration tests (requires network)
[group('test')]
test-with-integration *args:
    cargo test --all-features {{ args }}

# Run documentation tests (N/A for binary-only crates)
[group('test')]
test-doc:
    @echo "No doc tests available for binary-only crate"

# Run integration tests only (Linux/macOS)
[group('test')]
test-integration: build-release
    bash scripts/test-proxy.sh

# Run tests with nextest (faster, JUnit output)
[group('test')]
test-nextest:
    cargo nextest run --all-features --workspace --profile ci

# Run all tests (unit + integration)
[group('test')]
test-all: test test-integration

# ===== Coverage Tasks =====

# Get coverage info with llvm-cov
[group('coverage')]
coverage *args:
    cargo llvm-cov --all-features --workspace {{ args }}

# Generate HTML coverage report
[group('coverage')]
coverage-html:
    cargo llvm-cov --html
    @echo "Coverage report -> {{ coverage_dir }}/index.html"

# Show coverage summary in terminal
[group('coverage')]
coverage-text:
    cargo llvm-cov

# Generate coverage with nextest (for CI)
[group('coverage')]
coverage-nextest:
    cargo llvm-cov nextest --all-features --workspace --codecov --output-path codecov.json
    @echo "Coverage report -> codecov.json"

# ===== Linting & Formatting =====

# Format code
[group('lint')]
fmt *args:
    cargo fmt {{ args }}

# Check code formatting
[group('lint')]
fmt-check:
    cargo fmt --check --message-format short

# Run clippy linter with all features
[group('lint')]
clippy *args:
    cargo clippy --all-features --all-targets {{ args }}

# Run clippy linter with strict settings
[group('lint')]
clippy-strict *args:
    cargo clippy --all-features --all-targets -- -W clippy::cargo -W clippy::nursery -W clippy::pedantic {{ args }}

# Run all checks (fmt, clippy, tests)
[group('lint')]
check: fmt-check clippy test

# ===== Run Tasks =====

# Run proxy in debug mode (MCP server)
[group('run')]
run *args:
    RUST_LOG=debug cargo run {{ args }}

# Run proxy in release mode
[group('run')]
run-release: build-release
    RUST_LOG=info {{ build_dir }}/{{ binary_name }}

# ===== CLI Search Tasks =====

# Search Bun docs (usage: just search "Bun.serve")
[group('search')]
search query:
    cargo run -- --search "{{ query }}" | jq -r '.[]?[1]?.text'

# Search using release binary
[group('search')]
search-release query: build-release
    {{ build_dir }}/{{ binary_name }} --search "{{ query }}" | jq -r '.[]?[1]?.text'

# ===== Manual MCP Protocol Tests =====

# Test MCP initialize method
[group('mcp-test')]
test-mcp-init: build-release
    echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}' | {{ build_dir }}/{{ binary_name }}

# Test MCP tools/list method
[group('mcp-test')]
test-mcp-tools-list: build-release
    echo '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | {{ build_dir }}/{{ binary_name }}

# Test MCP tools/call method
[group('mcp-test')]
test-mcp-tools-call: build-release
    echo '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"SearchBun","arguments":{"query":"Bun.serve"}}}' | {{ build_dir }}/{{ binary_name }}

# Test MCP resources/list method
[group('mcp-test')]
test-mcp-resources-list: build-release
    echo '{"jsonrpc":"2.0","id":1,"method":"resources/list"}' | {{ build_dir }}/{{ binary_name }}

# Test MCP resources/read method
[group('mcp-test')]
test-mcp-resources-read: build-release
    echo '{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"bun://docs?query=Bun.serve"}}' | {{ build_dir }}/{{ binary_name }}

# Run all manual MCP protocol tests
[group('mcp-test')]
test-mcp-all: test-mcp-init test-mcp-tools-list test-mcp-tools-call test-mcp-resources-list test-mcp-resources-read

# ===== Cross-Platform Builds (Native) =====

# Build for Linux x86_64 (GNU)
[group('cross-build')]
build-linux-gnu:
    cargo build --release --target x86_64-unknown-linux-gnu

# Build for macOS Intel
[group('cross-build')]
build-macos-intel:
    cargo build --release --target x86_64-apple-darwin

# Build for macOS Apple Silicon
[group('cross-build')]
build-macos-arm:
    cargo build --release --target aarch64-apple-darwin

# Build for Windows x86_64
[group('cross-build')]
build-windows:
    cargo build --release --target x86_64-pc-windows-msvc

# Build for Windows ARM64
[group('cross-build')]
build-windows-arm:
    cargo build --release --target aarch64-pc-windows-msvc

# Build for Linux ARM64 (cross-compile with Zig)
[group('cross-build')]
build-linux-arm64:
    cargo zigbuild --release --target aarch64-unknown-linux-gnu

# Build for Linux x86_64 musl (static)
[group('cross-build')]
build-linux-musl:
    cargo zigbuild --release --target x86_64-unknown-linux-musl

# Build for Linux ARM64 musl (static)
[group('cross-build')]
build-linux-arm64-musl:
    cargo zigbuild --release --target aarch64-unknown-linux-musl

# Build for all native platforms (current OS only)
[group('cross-build')]
[confirm("Build for ALL platforms? This will take several minutes.")]
build-all-native: build-linux-gnu

# Build for all cross-compilation targets (requires Zig)
[group('cross-build')]
[confirm("Build for ALL platforms? This will take several minutes.")]
build-all-cross: build-linux-arm64 build-linux-musl build-linux-arm64-musl

# ===== Release Packaging =====

# Package Linux x86_64 binary as tar.gz
[group('package')]
package-linux: build-linux-gnu
    cd target/x86_64-unknown-linux-gnu/release && tar czf {{ binary_name }}-linux-x86_64.tar.gz {{ binary_name }}

# Package Linux ARM64 binary as tar.gz
[group('package')]
package-linux-arm64: build-linux-arm64
    cd target/aarch64-unknown-linux-gnu/release && tar czf {{ binary_name }}-linux-aarch64.tar.gz {{ binary_name }}

# Package Linux x86_64 musl binary as tar.gz
[group('package')]
package-linux-musl: build-linux-musl
    cd target/x86_64-unknown-linux-musl/release && tar czf {{ binary_name }}-linux-x86_64-musl.tar.gz {{ binary_name }}

# Package Linux ARM64 musl binary as tar.gz
[group('package')]
package-linux-arm64-musl: build-linux-arm64-musl
    cd target/aarch64-unknown-linux-musl/release && tar czf {{ binary_name }}-linux-aarch64-musl.tar.gz {{ binary_name }}

# Package macOS Intel binary as tar.gz
[group('package')]
package-macos-intel: build-macos-intel
    cd target/x86_64-apple-darwin/release && tar czf {{ binary_name }}-macos-x86_64.tar.gz {{ binary_name }}

# Package macOS Apple Silicon binary as tar.gz
[group('package')]
package-macos-arm: build-macos-arm
    cd target/aarch64-apple-darwin/release && tar czf {{ binary_name }}-macos-aarch64.tar.gz {{ binary_name }}

# Package Windows x86_64 binary as zip
[group('package')]
package-windows: build-windows
    cd target/x86_64-pc-windows-msvc/release && 7z a {{ binary_name }}-windows-x86_64.zip {{ binary_name }}.exe

# Package Windows ARM64 binary as zip
[group('package')]
package-windows-arm: build-windows-arm
    cd target/aarch64-pc-windows-msvc/release && 7z a {{ binary_name }}-windows-aarch64.zip {{ binary_name }}.exe

# Generate SHA256SUMS for all packaged binaries
[group('package')]
checksums:
    find target -type f \( -name "*.tar.gz" -o -name "*.zip" \) -exec sha256sum {} \; | sed 's|target/[^/]*/release/||' > SHA256SUMS
    cat SHA256SUMS

# ===== Development Tools =====

# Install development tools (llvm-cov, nextest, watch, zigbuild)
[group('dev')]
install-tools:
    rustup component add llvm-tools-preview
    cargo install cargo-llvm-cov cargo-nextest cargo-watch cargo-zigbuild

# Watch for changes and run tests
[group('dev')]
watch:
    cargo watch -x test

# Watch for changes and run clippy + tests
[group('dev')]
watch-check:
    cargo watch -x clippy -x test

# Development mode - auto-rebuild on changes
[group('dev')]
dev:
    cargo watch -x build

# ===== CI/CD Simulation =====

# Run CI checks locally (matches GitHub Actions)
[group('ci')]
ci: build test test-integration fmt-check clippy

# Run CI coverage workflow locally
[group('ci')]
ci-coverage: test-nextest coverage-nextest

# Run CI lint workflow locally
[group('ci')]
ci-lint: fmt-check clippy

# Run complete CI pipeline locally
[group('ci')]
ci-all: ci ci-coverage ci-lint

# ===== Version Management =====

# Show current version from Cargo.toml
[group('version')]
version:
    @echo "Version {{ current_version }}"

# Bump patch version (0.2.1 -> 0.2.2)
[group('version')]
bump-patch:
    cargo set-version --bump patch

# Bump minor version (0.2.1 -> 0.3.0)
[group('version')]
[confirm("Bump minor version (new features)?")]
bump-minor:
    cargo set-version --bump minor

# Bump major version (0.2.1 -> 1.0.0)
[group('version')]
[confirm("Bump major version? This is a BREAKING change!")]
bump-major:
    cargo set-version --bump major

# ===== Benchmarking & Profiling =====

# Run benchmarks (if any exist)
[group('bench')]
bench *args:
    cargo bench {{ args }}

# Show binary size
[group('bench')]
size: build-release
    ls -lh {{ build_dir }}/{{ binary_name }} | awk '{print "Binary size:", $5}'
    du -h {{ build_dir }}/{{ binary_name }}

# ===== Documentation =====

# Generate and open documentation
[group('doc')]
doc:
    cargo doc --open --no-deps

# Generate documentation including private items
[group('doc')]
[private]
_doc-private:
    cargo doc --no-deps --document-private-items

# Build documentation for GitHub Pages (with redirect)
[group('doc')]
doc-pages: _doc-private
    echo '<meta http-equiv="refresh" content="0; url=bun_docs_mcp_proxy/index.html">' > {{ doc_dir }}/index.html
    @echo "Documentation -> {{ doc_dir }}/index.html"

# Build and open documentation for GitHub Pages
[group('doc')]
doc-pages-open: doc-pages
    #!/usr/bin/env sh
    case "$(uname)" in \
        Linux)  xdg-open {{ doc_dir }}/index.html ;; \
        Darwin) open {{ doc_dir }}/index.html ;; \
        *)      start {{ doc_dir }}/index.html ;; \
    esac
