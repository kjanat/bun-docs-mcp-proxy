# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Rust-based tool with dual modes:
1. **MCP Server**: Protocol adapter bridging stdio-based MCP clients (like Zed) with HTTP/SSE-based Bun docs API
2. **CLI Tool**: Direct documentation search with formatted output (JSON/text/markdown) to terminal or file

## Essential Commands

### Build & Test

```bash
# Build optimized release binary
cargo build --release

# Run all tests
cargo test

# Run tests with Makefile
make test

# Generate coverage report (uses cargo-llvm-cov)
make coverage

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

### CLI Mode

```bash
# Search and output to terminal (JSON format)
./target/release/bun-docs-mcp-proxy --search "Bun.serve"

# Plain text format
./target/release/bun-docs-mcp-proxy -s "HTTP server" -f text

# Markdown format
./target/release/bun-docs-mcp-proxy -s "WebSocket" -f markdown

# Save to file
./target/release/bun-docs-mcp-proxy -s "Bun.serve" -o docs.json

# Text format to file
./target/release/bun-docs-mcp-proxy -s "testing" -f text -o output.txt
```

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

- `src/main.rs` - Dual-mode entrypoint:
  - **CLI mode** (if `--search` flag): Direct search with formatted output
  - **MCP mode** (default): Event loop reading stdin → dispatch → write stdout
  - Handles `initialize`, `tools/list`, `tools/call`, `resources/list`, `resources/read`
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
# All tests
cargo test

# With Makefile
make test

# Coverage report (uses cargo-llvm-cov)
make coverage

# Coverage summary
make coverage-text

# Run with cargo-nextest (faster test runner with JUnit output)
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

## CLI Usage

The binary supports both MCP server mode (default) and CLI search mode:

**CLI Flags**:
- `-s, --search <QUERY>` - Enable CLI mode with search query
- `-o, --output <FILE>` - Write output to file (default: stdout)
- `-f, --format <FORMAT>` - Output format: `json` (default), `text`, `markdown`
- `-h, --help` - Show help
- `-V, --version` - Show version

**Output Formats**:
- `json` - Pretty-printed JSON with full API response
- `text` - Plain text extraction of content (strips metadata)
- `markdown` - Markdown-formatted with heading and content blocks

**CLI vs MCP Mode**:
- CLI mode: Activated by `--search` flag, outputs directly, exits after search
- MCP mode: Default when no `--search` flag, runs event loop on stdin/stdout

## Common Issues

**Binary size increased**: Check release profile settings in `Cargo.toml`. Verify `strip = true`, `opt-level = "z"`, and
`lto = true`.

**SSE parsing fails**: Bun Docs API may have changed response format. Check `src/http/mod.rs:85` for result/error field
detection logic.

**Timeout on tests**: Default HTTP timeout is 5s (`REQUEST_TIMEOUT_SECS`). Network issues or Bun API slowness may
require adjustment.

**Cross-compilation fails**: Ensure target toolchain installed with `rustup target add <target-triple>`.

**CLI search returns empty**: Verify network connectivity to `https://bun.com/docs/mcp`. Check RUST_LOG=debug output for errors.
