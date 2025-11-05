# Bun Docs MCP Proxy - Project Index

> **Version**: 0.3.0  
> **Repository**: [kjanat/bun-docs-mcp-proxy](https://github.com/kjanat/bun-docs-mcp-proxy)  
> **License**: MIT

Native Rust MCP proxy bridging stdio-based clients (Zed, Claude Code) with Bun's HTTP documentation server.

---

## 📚 Quick Navigation

### Getting Started

- [**README.md**](./README.md) - Main documentation, features, usage
- [**CLAUDE.md**](./CLAUDE.md) - Project overview for Claude Code
- [**CHANGELOG.md**](./CHANGELOG.md) - Version history and release notes
- [**LICENSE**](./LICENSE) - MIT license terms

### Development

- [**CONTRIBUTING.md**](./.github/CONTRIBUTING.md) - Contribution guidelines
- [**TESTING.md**](./.github/TESTING.md) - Testing strategy and workflows
- [**IMPLEMENTATION_SUMMARY.md**](./IMPLEMENTATION_SUMMARY.md) - Implementation details
- [**AGENTS.md**](./AGENTS.md) - AI agent collaboration patterns

### Security & Quality

- [**SECURITY.md**](./.github/SECURITY.md) - Security policy and vulnerability reporting
- [**Pull Request Template**](./.github/pull_request_template.md) - PR guidelines
- [**Taskfile.yml**](./Taskfile.yml) - Build automation and task definitions

---

## 🏗️ Architecture

### Core Modules

```text
src/
├── main.rs         # Event loop, MCP method handlers
├── protocol.rs     # JSON-RPC 2.0 types and builders
├── transport.rs    # Stdio transport (async stdin/stdout)
└── http.rs         # HTTP client with SSE parsing
```

**Request Flow**:

```text
stdin (JSON-RPC) → Proxy → HTTP POST → bun.com/docs/mcp → SSE stream → parse → stdout (JSON-RPC)
```

### Module Responsibilities

| Module           | Responsibility              | Key Types                                           |
| ---------------- | --------------------------- | --------------------------------------------------- |
| **main.rs**      | Event loop, method dispatch | Handler functions                                   |
| **protocol.rs**  | JSON-RPC serialization      | `JsonRpcRequest`, `JsonRpcResponse`, `JsonRpcError` |
| **transport.rs** | Async I/O                   | `StdioTransport`                                    |
| **http.rs**      | HTTP + SSE                  | `BunDocsClient`, `SearchQuery`                      |

### Testing Structure

```text
tests/
├── integration_test.rs   # Protocol compliance
├── http_edge_cases.rs    # HTTP error handling
├── cli_args.rs           # CLI argument parsing
└── cli_integration.rs    # CLI search mode
```

**Test Coverage**: ~86% (41 unit tests, 5 integration tests)

---

## 🚀 Common Tasks

### Build & Test

```bash
# Quick development
task br          # Build release binary
task t           # Run unit tests
task c           # Run all checks (fmt + clippy + tests)

# Coverage
task cov         # Generate HTML coverage report
task covt        # Terminal coverage summary

# CI simulation
task ci          # Run CI checks locally
task ci-lint     # Lint workflow
task ci-coverage # Coverage workflow
```

### Manual Testing

```bash
# Individual MCP methods
task test-mcp-init           # Initialize
task test-mcp-tools-list     # List tools
task test-mcp-tools-call     # Call SearchBun
task test-mcp-resources-list # List resources
task test-mcp-resources-read # Read resource

# All methods
task test-mcp-all
```

### CLI Search Mode

```bash
# Search with different formats
bun-docs-mcp-proxy -s "Bun.serve" -f json
bun-docs-mcp-proxy -s "WebSocket" -f markdown -o ws-docs.md
bun-docs-mcp-proxy -s "test" -f text

# Using Task
task search -- "your query"
task sr -- "your query"  # Release binary
```

---

## 📖 API Reference

### Supported MCP Methods

| Method             | Description          | Params                               | Response                          |
| ------------------ | -------------------- | ------------------------------------ | --------------------------------- |
| **initialize**     | Protocol handshake   | `capabilities`, `clientInfo`         | Server info, version `2024-11-05` |
| **tools/list**     | List available tools | None                                 | `SearchBun` tool definition       |
| **tools/call**     | Execute SearchBun    | `name`, `arguments.query`            | Search results array              |
| **resources/list** | List resources       | None                                 | `bun://docs` resource             |
| **resources/read** | Read by URI          | `uri` (e.g., `bun://docs?query=...`) | Resource content                  |

### CLI Arguments

```bash
bun-docs-mcp-proxy [OPTIONS]

Options:
  -s, --search <QUERY>     Search Bun documentation
  -f, --format <FORMAT>    Output format: json|text|markdown [default: json]
  -o, --output <FILE>      Save to file instead of stdout
  -h, --help              Print help
  -V, --version           Print version
```

---

## 🔧 Configuration

### Release Profile (Cargo.toml)

```toml
[profile.release]
opt-level = "z"        # Size optimization (2.7 MB binary)
strip = true           # Remove debug symbols
lto = true            # Link-time optimization
panic = "abort"       # Smaller panic handler
codegen-units = 1     # Better optimization
```

### Feature Flags

```toml
[features]
default = []
integration-tests = []  # Enable real API tests
```

### Environment Variables

```bash
RUST_LOG=debug     # Enable debug logging
RUST_LOG=trace     # Very verbose logging
```

---

## 🧪 Testing Strategy

### Dual Testing Approach

1. **Unit Tests** (Default) - Fast, mocked, no network
   - 41 tests across 4 modules
   - Uses `mockito` for HTTP mocking
   - Runs in ~2-3 seconds

2. **Integration Tests** (Opt-in) - Real API calls
   - 5 tests with live `bun.com` API
   - Requires `--features integration-tests`
   - Runs daily in CI + manual triggers

### Test Execution

```bash
# Unit tests only
task t  # or: cargo test

# Integration tests
task tio  # or: cargo test --features integration-tests

# All tests
task twi  # or: cargo test --all-features

# With nextest (faster)
task tn
```

### Coverage Tooling

- **Primary**: `cargo-llvm-cov` (accurate async coverage)
- **CI**: Codecov integration
- **Formats**: HTML, JSON (codecov), LCOV, Cobertura

---

## 🌐 Cross-Platform Builds

### Native Targets

| Platform            | Target Triple              | Task Command             |
| ------------------- | -------------------------- | ------------------------ |
| Linux x86_64 (GNU)  | `x86_64-unknown-linux-gnu` | `task build-linux-gnu`   |
| macOS Intel         | `x86_64-apple-darwin`      | `task build-macos-intel` |
| macOS Apple Silicon | `aarch64-apple-darwin`     | `task build-macos-arm`   |
| Windows x86_64      | `x86_64-pc-windows-msvc`   | `task build-windows`     |

### Cross-Compilation (Zig)

Requires `cargo-zigbuild`:

```bash
task install-tools  # Installs cargo-zigbuild

task build-linux-arm64        # ARM64 Linux
task build-linux-musl         # Static x86_64 musl
task build-linux-arm64-musl   # Static ARM64 musl
```

### Batch Builds

```bash
task build-all-native  # All native platforms
task build-all-cross   # All cross-compilation targets
```

---

## 📊 Performance Metrics

**Measured on Linux x86_64 (Manjaro 6.16.12)**:

| Metric       | Value   | Target  | Status           |
| ------------ | ------- | ------- | ---------------- |
| Binary Size  | 2.7 MB  | < 5 MB  | ✅ 46% under     |
| Startup Time | 4 ms    | < 10 ms | ✅ 60% faster    |
| Memory Usage | ~2-5 MB | < 10 MB | ✅ Within target |
| Request Time | ~400ms  | N/A     | Network-bound    |

### vs TypeScript Proxy

| Metric       | TypeScript (Bun) | Rust Native | Improvement       |
| ------------ | ---------------- | ----------- | ----------------- |
| Binary Size  | ~50 MB           | 2.7 MB      | **95% smaller**   |
| Startup      | ~100-200 ms      | 4 ms        | **25-50x faster** |
| Memory       | ~30-50 MB        | ~2-5 MB     | **10x less**      |
| Runtime Deps | Bun/Node.js      | None        | ✅ Standalone     |

---

## 🔐 Security

### Vulnerability Reporting

See [SECURITY.md](.github/SECURITY.md) for reporting procedures.

### Security Features

- **TLS**: Uses `rustls-tls` (no OpenSSL dependency)
- **Error Handling**: Proper JSON-RPC error codes
- **Input Validation**: URI parsing and query sanitization
- **Dependency Auditing**: Dependabot enabled

---

## 🤝 Contributing

### Quick Start

1. Fork and clone
2. Install tools: `task install-tools`
3. Run checks: `task c`
4. Submit PR (see [CONTRIBUTING.md](./.github/CONTRIBUTING.md))

### Development Workflow

```bash
# Watch mode (auto-rebuild)
task dev

# Run CI checks locally
task ci

# Version bumps
task bump-patch  # 0.3.0 → 0.3.1
task bump-minor  # 0.3.0 → 0.4.0
task bump-major  # 0.3.0 → 1.0.0
```

### Code Quality

- **Formatting**: `task fmt-check`
- **Linting**: `task clippy` (nursery + pedantic lints)
- **Tests**: `task t` (required before commit)
- **Coverage**: Maintain ≥80% coverage

---

## 📦 Dependencies

### Core Runtime

| Crate                  | Purpose        | Features                                 |
| ---------------------- | -------------- | ---------------------------------------- |
| **tokio**              | Async runtime  | rt-multi-thread, io-std, io-util, macros |
| **reqwest**            | HTTP client    | json, stream, rustls-tls                 |
| **eventsource-stream** | SSE parsing    | (default)                                |
| **serde_json**         | JSON           | (default)                                |
| **anyhow**             | Error handling | (default)                                |
| **tracing**            | Logging        | (default)                                |

### Development

| Crate          | Purpose            |
| -------------- | ------------------ |
| **mockito**    | HTTP mocking       |
| **assert_cmd** | CLI testing        |
| **predicates** | Test assertions    |
| **tempfile**   | Temp file handling |

---

## 🔗 Related Projects

- [**bun-docs-mcp-zed**](https://github.com/kjanat/bun-docs-mcp-zed) - Zed extension consuming this proxy
- [**Bun Documentation**](https://bun.sh/docs) - Official Bun docs
- [**MCP Specification**](https://modelcontextprotocol.io) - Model Context Protocol

---

## 📝 Version History

See [CHANGELOG.md](./CHANGELOG.md) for detailed version history.

**Current**: v0.3.0 (CLI search mode with markdown format)

### Recent Changes

- **v0.3.0**: CLI search mode, markdown format with MDX fetching
- **v0.2.1**: MCP resources support, URI-based search
- **v0.2.0**: Dual testing strategy, improved CI
- **v0.1.0**: Initial release

---

## 🛠️ Tooling

### Build Automation

- **Task** - Primary build tool (see [Taskfile.yml](./Taskfile.yml))
- **Cargo** - Rust build system
- **GitHub Actions** - CI/CD

### CI/CD Workflows

- [**ci.yml**](./.github/workflows/ci.yml) - Build, test, lint on every push
- [**release.yml**](./.github/workflows/release.yml) - Cross-platform binary releases
- [**integration-tests.yml**](./.github/workflows/integration-tests.yml) - Daily API tests
- [**pages.yml**](./.github/workflows/pages.yml) - Documentation deployment
- [**claude.yml**](./.github/workflows/claude.yml) - AI code review

### Development Tools

```bash
task install-tools  # Installs:
# - cargo-llvm-cov (coverage)
# - cargo-nextest (faster testing)
# - cargo-watch (file watching)
# - cargo-zigbuild (cross-compilation)
# - llvm-tools-preview (coverage backend)
```

---

## 📚 Additional Resources

### Documentation Files

- **CLAUDE.md** - Claude Code project instructions
- **AGENTS.md** - AI agent collaboration patterns
- **IMPLEMENTATION_SUMMARY.md** - Implementation details
- **TESTING.md** - Comprehensive testing guide

### GitHub Templates

- **Bug Report** - `.github/ISSUE_TEMPLATE/bug_report.yml`
- **Feature Request** - `.github/ISSUE_TEMPLATE/feature_request.yml`
- **Pull Request** - `.github/pull_request_template.md`

### Configuration Files

- **Taskfile.yml** - Task definitions (60+ tasks)
- **Cargo.toml** - Package manifest and dependencies
- **.github/dependabot.yml** - Dependency updates
- **.config/nextest.toml** - Test runner configuration
- **.pre-commit-config.yaml** - Pre-commit hooks

---

## 🎯 Project Goals

1. **Zero runtime dependencies** - Single native binary
2. **Minimal size** - < 5 MB with full TLS support
3. **Fast startup** - < 10 ms cold start
4. **Low memory** - < 10 MB RSS
5. **Standard protocols** - JSON-RPC 2.0 + SSE
6. **Production quality** - ≥80% test coverage, comprehensive error handling

---

## 📊 Project Statistics

- **Language**: Rust (Edition 2024)
- **Lines of Code**: ~2,000 (excluding tests)
- **Test Coverage**: ~86%
- **Dependencies**: 8 runtime, 4 dev
- **Supported Platforms**: 8 targets (4 native + 4 cross-compile)
- **GitHub Actions**: 6 workflows
- **Documentation Files**: 10+ markdown files
- **Task Definitions**: 60+ automated tasks

---

## 🗺️ Directory Structure

```text
bun-docs-mcp-proxy/
├── src/                      # Source code
│   ├── main.rs              # Event loop and handlers
│   ├── main_tests.rs        # Main module tests
│   ├── protocol.rs          # JSON-RPC types
│   ├── transport.rs         # Stdio transport
│   └── http.rs              # HTTP + SSE client
├── tests/                    # Integration tests
│   ├── integration_test.rs  # Protocol compliance
│   ├── http_edge_cases.rs   # HTTP error handling
│   ├── cli_args.rs          # CLI parsing
│   └── cli_integration.rs   # CLI search mode
├── .github/                  # GitHub configuration
│   ├── workflows/           # CI/CD pipelines
│   ├── ISSUE_TEMPLATE/      # Issue templates
│   ├── CONTRIBUTING.md      # Contribution guide
│   ├── SECURITY.md          # Security policy
│   └── TESTING.md           # Testing documentation
├── scripts/                  # Utility scripts
│   ├── fetch-docs.rs        # Rust doc fetcher
│   └── fetch-docs.ts        # TypeScript doc fetcher
├── .config/                  # Tool configuration
│   └── nextest.toml         # Test runner config
├── Cargo.toml               # Package manifest
├── Cargo.lock               # Dependency lock
├── Taskfile.yml             # Build automation
├── README.md                # Main documentation
├── CLAUDE.md                # Claude Code instructions
├── CHANGELOG.md             # Version history
├── INDEX.md                 # This file
└── LICENSE                  # MIT license
```

---

**Last Updated**: 2025-11-05  
**Maintainer**: [@kjanat](https://github.com/kjanat)
