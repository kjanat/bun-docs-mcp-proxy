# AGENTS.md

Guidance for AI coding agents working in this Rust MCP proxy codebase.

## Project Overview

Rust MCP (Model Context Protocol) proxy bridging stdio-based clients (Zed) with
Bun's HTTP/SSE docs API at `https://bun.com/docs/mcp`.\
Receives JSON-RPC 2.0 on stdin, forwards to HTTP, parses SSE, returns
JSON-RPC on stdout.

## Build Commands

```bash
# Build (recommended - uses just)
just build           # Debug build with all features
just br              # Release build (alias: just release)

# Raw cargo
cargo build --release
cargo build --all-features --all-targets
```

## Test Commands

```bash
# Run all tests
just t               # All unit tests (fast, no network)
cargo test           # Same as above

# Run single test
cargo test test_name                        # By name substring
cargo test protocol::tests::deserialize     # By module path
cargo test --test integration_test          # Single test file

# Test categories
just tu              # Unit tests only (--bins)
just ti              # Integration tests (shell script)
just tio             # Integration with real API (--features integration-tests)
just tn              # Nextest (faster, JUnit output)

# With output
cargo test -- --nocapture                   # Show println! output
cargo test -- --show-output                 # Show test output on failure
RUST_LOG=debug cargo test                   # With tracing logs
```

## Lint & Format

```bash
just c               # Full check: fmt-check + clippy + tests
just fc              # Format check only
just lint            # Clippy with all features
just lint-strict     # Clippy with all lint groups enabled

# Formatting (dprint is the primary formatter)
dprint fmt           # Format all files (ts, json, md, yaml, toml, rs)
dprint check         # Check formatting without changes
cargo fmt            # Rust only (also called by dprint)
cargo clippy --all-features --all-targets
```

**dprint** (`.dprint.jsonc`) orchestrates formatting for all file types:

- **Rust**: via `rustfmt` (exec plugin)
- **TOML**: via `tombi` (exec plugin)
- **TypeScript/JSON/Markdown/YAML/HTML/CSS**: native dprint plugins

## Coverage

```bash
just cov             # Generate coverage (llvm-cov)
just covh            # HTML report -> target/llvm-cov/html/
just covt            # Terminal summary
```

## Code Style Guidelines

### Imports

Grouped by `rustfmt.toml` settings:

1. `std` crate
2. External crates
3. Crate-local modules

```rust
use anyhow::{Context as _, Result}; // Std/external first
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::protocol::JsonRpcResponse; // Local modules last
```

Use `as _` for trait imports only needed for methods (e.g., `Context as _`).

### Formatting (rustfmt.toml)

- **Max width**: 100 chars
- **Indent**: 4 spaces
- **Newlines**: Unix (LF)
- **Imports**: Crate-level granularity, grouped std/external/crate

### Types & Naming

- **Structs**: `PascalCase` - `JsonRpcRequest`, `BunDocsClient`
- **Functions**: `snake_case` - `forward_request`, `parse_sse_response`
- **Constants**: `SCREAMING_SNAKE_CASE` - `REQUEST_TIMEOUT_SECS`, `MAX_RETRIES`
- **Type suffixes**: Use `_SECS`, `_MS`, `_SIZE` for clarity

### Error Handling

Use `anyhow` for application errors with context:

```rust
use anyhow::{Context as _, Result};

fn example() -> Result<Value> {
    let response = client.get(url).await.context("Failed to send request")?; // Add context

    response
        .json()
        .await
        .context("Failed to parse JSON response")
}
```

Return `Result<T, String>` for simple validation errors in helpers.

### Clippy Configuration (Cargo.toml)

Project uses strict clippy with `pedantic`, `nursery`, and `cargo` groups enabled. Key allowances:

```toml
# Allowed (idiomatic Rust)
implicit_return    = "allow"  # fn foo() -> i32 { 42 }
question_mark_used = "allow"  # ? operator
shadow_reuse       = "allow"  # let line = line.trim()
expect_used        = "allow"  # .expect("msg") over .unwrap()
# Warned
shadow_unrelated   = "warn"   # Shadowing with different type/meaning
mod_module_files   = "warn"   # Prefer single-file modules
```

### Documentation

- Module-level `//!` docs for each file
- Function docs with `///` for public APIs
- Include `# Arguments`, `# Returns`, `# Errors` sections
- Use `#[must_use]` for functions returning values that shouldn't be ignored

### Test Organization

Tests live in `#[cfg(test)] mod tests` at bottom of each file:

```rust
#[cfg(test)]
#[allow(clippy::expect_used)] // Tests can use expect()
#[allow(clippy::unwrap_used)] // Tests can use unwrap()
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_name() {
        // Arrange
        let input = json!({"key": "value"});

        // Act
        let result = function_under_test(input);

        // Assert
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn async_test() {
        // For async tests, use mockito for HTTP mocking
        let mut server = mockito::Server::new_async().await;
        let mock = server.mock("POST", "/").create_async().await;
        // ...
    }
}
```

### Async Patterns

- Use `tokio` runtime with `#[tokio::main]` / `#[tokio::test]`
- Prefer `async fn` over manual `Future` implementations
- Use `tokio::time::sleep` for delays (not `std::thread::sleep`)

### Logging

Log to stderr (stdout reserved for JSON-RPC):

```rust
use tracing::{debug, error, info, warn};

debug!("Detailed info: {}", value); // RUST_LOG=debug
info!("Normal operation: {}", msg); // Default level
warn!("Recoverable issue: {}", err);
error!("Fatal error: {}", err);
```

## Architecture Reference

```tree
src/
  main.rs      - CLI parsing, event loop, JSON-RPC handlers
  http.rs      - BunDocsClient, SSE parsing, retry logic
  protocol.rs  - JsonRpcRequest/Response/Error types
  transport.rs - StdioTransport for stdin/stdout I/O
tests/
  integration_test.rs  - Protocol compliance tests
  cli_integration.rs   - CLI argument tests
  cli_args.rs          - Argument parsing tests
  http_edge_cases.rs   - HTTP error handling tests
```

## Quick Reference

| Task          | Command                |
| ------------- | ---------------------- |
| Build release | `just br`              |
| Run tests     | `just t`               |
| Single test   | `cargo test test_name` |
| Format        | `dprint fmt`           |
| Lint          | `just lint`            |
| Full check    | `just c`               |
| Coverage      | `just cov`             |
| Run debug     | `just run`             |
