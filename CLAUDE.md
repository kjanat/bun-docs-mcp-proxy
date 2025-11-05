# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Rust-based MCP (Model Context Protocol) proxy that bridges stdio-based MCP clients (like Zed) with the HTTP/SSE-based
Bun documentation server at `https://bun.com/docs/mcp`. Acts as a protocol adapter: receives JSON-RPC 2.0 over stdin,
forwards to Bun Docs HTTP API, parses SSE responses, and returns JSON-RPC over stdout.

## Essential Commands

### Task-based Workflow (Recommended)

This project uses [Task](https://taskfile.dev) for build automation with GitHub Actions integration.

```bash
# Quick start - Common tasks
task br          # Build release binary
task t           # Run all tests
task c           # Run all checks (fmt + clippy + tests)
task cov         # Generate HTML coverage report

# CI simulation (matches GitHub Actions)
task ci          # Run CI checks locally
task ci-lint     # Run lint checks
task ci-coverage # Run coverage workflow

# Development
task dev         # Watch mode (auto-rebuild on changes)
task run         # Run proxy in debug mode

# Version management (with safety prompts)
task bump-patch  # Bump patch version (0.2.1 → 0.2.2)
task bump-minor  # Bump minor version (0.2.1 → 0.3.0) [prompted]
task bump-major  # Bump major version (0.2.1 → 1.0.0) [prompted]

# List all available tasks
task --list-all
```

**CI Environment**: In CI/CD pipelines, use `--yes` flag to skip prompts:

```bash
task --yes clean        # Auto-confirm in CI
task --yes bump-major   # Skip breaking change prompt
```

**GitHub Actions Integration**: Tasks automatically use collapsible output groups (`::group::`) in GitHub Actions for cleaner CI logs.

### Build & Test (Raw Commands)

```bash
# Build optimized release binary
cargo build --release

# Run all tests
cargo test

# Run tests with Task
task t

# Generate coverage report (uses cargo-llvm-cov)
task cov

# Run with debug logging
RUST_LOG=debug ./target/release/bun-docs-mcp-proxy

# Manual test of tools/call
echo '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"SearchBun","arguments":{"query":"Bun.serve"}}}' | \
./target/release/bun-docs-mcp-proxy

# Manual test of resources/read
echo '{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"bun://docs?query=Bun.serve"}}' | \
./target/release/bun-docs-mcp-proxy

# Manual test of resources/list
echo '{"jsonrpc":"2.0","id":1,"method":"resources/list"}' | \
./target/release/bun-docs-mcp-proxy
```

### CLI Search Mode

The proxy can operate in CLI mode for direct documentation searches with various output formats:

```bash
# Search with default JSON output
bun-docs-mcp-proxy --search "Bun.serve"

# Save results as markdown (fetches raw MDX sources)
bun-docs-mcp-proxy -s "HTTP server" -f markdown -o results.md

# Export as JSON for processing
bun-docs-mcp-proxy --search "WebSocket" --format json --output ws-docs.json

# Plain text output
bun-docs-mcp-proxy -s "test" -f text
```

**Output Formats**:

- **JSON** (`--format json`): Structured data export, pretty-printed for readability
- **Text** (`--format text`): Plain text extraction from search results
- **Markdown** (`--format markdown`): **Fetches raw MDX source files** from documentation URLs
  - Parses `Link:` fields from search results
  - Makes HTTP GET requests with `Accept: text/markdown` header
  - Aggregates multiple documents with `---` separators
  - Includes `<!-- Source: URL -->` comments for traceability
  - Falls back to original text if fetch fails (with `<!-- Error: ... -->` comment)

**Breaking Change (v0.3.0)**: The markdown format now fetches raw MDX sources instead of just formatting search result text. This provides access to the full documentation content including MDX components.

### Cross-Platform Builds

```bash
# Linux ARM64
cargo build --release --target aarch64-unknown-linux-gnu

# macOS Intel
cargo build --release --target x86_64-apple-darwin

# macOS Apple Silicon
cargo build --release --target aarch64-apple-darwin

# Windows
cargo build --release --target x86_64-pc-windows-msvc
```

## Architecture

**Request Flow**: stdin (JSON-RPC) → Proxy → HTTP POST → bun.com/docs/mcp → SSE stream → parse → stdout (JSON-RPC)

**Module Breakdown**:

- `src/main.rs` - Event loop: read stdin → dispatch by method → write stdout. Handles `initialize`, `tools/list`,
  `tools/call`
- `src/protocol/` - JSON-RPC 2.0 types (`JsonRpcRequest`, `JsonRpcResponse`, `JsonRpcError`) with success/error builders
- `src/transport/` - `StdioTransport`: async line-based stdin reader + stdout writer with flush
- `src/http/` - `BunDocsClient`: HTTP client with SSE parser. Extracts `result` field from SSE data events

**Key Implementation Details**:

- SSE parsing uses `eventsource-stream` to parse Server-Sent Events from Bun Docs API
- JSON-RPC response is embedded in SSE data events as `{"result": {...}}` or `{"error": {...}}`
- `handle_tools_call` forwards entire request structure to preserve params/arguments
- Error codes: `-32700` (parse), `-32601` (method not found), `-32603` (internal/HTTP errors)
- Logs to stderr (Zed captures for extension logs), responses to stdout
- Uses `rustls-tls` (no OpenSSL dependency) for portable TLS

## Performance & Optimization

**Release Profile** (`Cargo.toml`):

- `opt-level = "z"` - Size optimization (currently 2.7 MB binary)
- `strip = true` - No debug symbols
- `lto = true` - Link-time optimization
- `panic = "abort"` - Smaller panic handler
- `codegen-units = 1` - Better optimization at cost of compile time

**Metrics**: 4ms startup, ~2-5 MB RSS, ~400ms request time (network-bound)

## Testing

**Test Coverage: X%** (X/X lines)

### Test Suite (46 tests)

**Unit Tests** (41 tests):

- `src/protocol/mod.rs` - JSON-RPC serialization/deserialization
- `src/http/mod.rs` - HTTP client, SSE parsing, mocked API tests
- `src/transport/mod.rs` - Stdio transport logic
- `src/main.rs` - Handler functions, error paths

**Integration Tests** (5 tests):

- `tests/integration_test.rs` - Protocol compliance, response structure validation

**Shell Integration Test**:

- `test-proxy.sh` - End-to-end proxy validation (requires `jq`)

### Running Tests

```bash
# With Task (Recommended)
task t           # Run all tests
task tu          # Run unit tests only
task ti          # Run integration tests only
task tn          # Run with nextest (faster, JUnit output)
task cov         # Generate HTML coverage report
task covt        # Show coverage summary in terminal

# Raw commands
cargo test
make test
cargo nextest run --all-features --workspace --profile ci
# JUnit report saved to target/nextest/ci/junit.xml
```

Tests use `cargo-llvm-cov` (not tarpaulin) for accurate async coverage.
Mock HTTP server tests use `mockito` for reliable async test execution.
CI uses `cargo-nextest` for faster test execution and JUnit XML reporting.

## Protocol Implementation

**Supported Methods**:

- `initialize` - Returns protocol version `2024-11-05`, capabilities (tools + resources), server info
- `tools/list` - Returns single tool: `SearchBun` with `query` string parameter
- `tools/call` - Forwards to Bun Docs API, extracts `result` from SSE response
- `resources/list` - Returns single resource: `bun://docs` with Bun Documentation
- `resources/read` - Reads resource by URI (e.g., `bun://docs?query=Bun.serve`)

**SSE Parsing Logic** (`src/http/mod.rs:68-106`):

- Streams response bytes through `eventsource-stream`
- Parses each event's data field as JSON
- Looks for `result` or `error` field to identify JSON-RPC response
- Breaks on first valid response (ignores subsequent events)
- Returns error if no valid response found in stream

## Common Issues

**Binary size increased**: Check release profile settings in `Cargo.toml`. Verify `strip = true`, `opt-level = "z"`, and
`lto = true`.

**SSE parsing fails**: Bun Docs API may have changed response format. Check `src/http/mod.rs:85` for result/error field
detection logic.

**Timeout on tests**: Default HTTP timeout is 5s (`REQUEST_TIMEOUT_SECS`). Network issues or Bun API slowness may
require adjustment.

**Cross-compilation fails**: Ensure target toolchain installed with `rustup target add <target-triple>`.

**CLI search returns empty**: Verify network connectivity to `https://bun.com/docs/mcp`. Check RUST_LOG=debug output for errors.

**Task prompts fail in CI**: Tasks with `prompt:` (clean, bump-major, bump-minor, build-all-\*) require `--yes` flag in non-interactive environments:

```bash
# CI/CD usage
task --yes clean
task --yes bump-major
```

**GitHub Actions logs verbose**: Task automatically groups output using `::group::` syntax. Expand/collapse groups in Actions UI for cleaner logs.
