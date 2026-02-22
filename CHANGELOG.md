# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog], and this project adheres to
[Semantic Versioning].

<!--
## [Unreleased]

Switch to typescript-based MCP proxy for easier maintenance and quicker
development.

DO NOT REMOVE THIS COMMENT!
-->

## [Unreleased]

### Added

- `deny.toml` for cargo-deny license/security policy
- Concurrency group in `release.yml` to prevent duplicate releases
- `*.profraw`, `*.profdata`, `tools/websockets.*` to `.gitignore`
- Reusable `rust-setup` composite action for CI (toolchain, cache, tools)
- `create-release` composite action (Node 24) for automated GitHub releases
- Dynamic build matrix in `release.yml` via `actions/github-script`
- `workflow_dispatch` + `merge_group` triggers for CI
- `workflow_dispatch` inputs (tag, draft, prerelease) for release workflow
- Build artifact uploads in CI workflow
- `pull_request` trigger for Claude code review (merged into `claude.yml`)

### Changed

- All CI workflows use shared `rust-setup` action (DRY)
- `autofix.yml` switched from `npm` to `bun` for formatter installation
- `ci.yml` switched from `paths-ignore` to explicit `paths` filter
- `ci.yml` test step uses `just test` instead of `just ci`
- `release.yml` build/package/attest steps consolidated into OS-agnostic blocks
- `integration-tests.yml` switched from `codecov/test-results-action@v1` to
  `codecov/codecov-action@v5`
- Claude code review merged from standalone workflow into `claude.yml`
- Claude model parameterized via `env.MODEL`
- Codecov uploads no longer use explicit token (tokenless)
- `dependabot.yml` compacted to inline YAML style
- `Cargo.toml` dependencies reformatted to `[dependencies.X]` table sections
- `Cargo.toml` lint groups moved to dedicated `[lints.clippy.X]` table sections
- dprint plugins updated (typescript 0.95.13 -> 0.95.15, markdown 0.20.0 ->
  0.21.1)
- dprint YAML `printWidth` 120 -> 150
- `CLAUDE.md` is now a symlink to `AGENTS.md`
- Moved JS/TS tooling to `tools/` directory (was root + `scripts/`)
- Expanded `bacon.toml` with project-specific jobs and keybindings
- Updated `SECURITY.md` supported versions (1.x supported, <1.0.0 unsupported)
- Updated `CONTRIBUTING.md` Rust version requirement (1.85.0+, edition 2024)

### Removed

- `claude-code-review.yml` (merged into `claude.yml`)
- `macos-15-intel` (x86_64-apple-darwin) from CI build matrix
- `jq` as a CI test dependency
- Redundant standalone `cargo test` run in `integration-tests.yml`

### Fixed

- SIGABRT coredumps when parent process (Zed) closes stderr pipe — reset SIGPIPE
  to SIG_DFL so broken-pipe writes terminate cleanly (exit 141) instead of
  panicking through `eprintln!`/tracing with `panic = "abort"`
- `AGENTS.md` module structure to match actual v1.0 layout
- Documentation links to deleted files (`INDEX.md`, `TESTING.md`)
- `CLAUDE.md` -> `AGENTS.md` references in templates

## [1.0.0] - 2026-01-24

### Added

- **Security hardening:**
  - Stream-limited error body reading to prevent memory DoS attacks
  - SSRF protection: allowlist `bun.com`/`bun.sh` for MDX fetches
  - SSE deadline timeout to prevent hanging on malicious streams
- **Performance:** Concurrent markdown fetching (up to 4 parallel requests)
- **Architecture:** Complete module restructure to library + binary layout:
  - `src/lib.rs` with `app/`, `mcp/`, `upstream/`, `format/`, `io/`, `util`
    modules
  - `src/bin/bun-docs-mcp-proxy.rs` as thin CLI wrapper
- Generic `Transport<R, W>` trait for testable I/O
- `BunDocsClientBuilder` with configurable timeout, retries, backoff
- Typed `UpstreamResponse` enum for proper error handling
- `JsonRpcEnvelope` for SSE response parsing
- `Method` enum replacing magic strings
- Tracing spans via `#[instrument]` throughout
- CODEOWNERS, issue/PR templates, and security policy
- Project index documentation for discoverability
- justfile with 50+ development recipes
- bacon.toml for continuous checking
- lefthook for pre-commit hooks

### Changed

- **JSON-RPC 2.0 compliance:**
  - Parse errors use `id: null`
  - Validate `jsonrpc == "2.0"` field (returns `-32600`)
  - Distinguish `id: null` from missing `id` (notification)
- Migrate all tests from `tests/` to inline modules
- Add AGENTS.md with project guidance for Claude Code
- Normalize formatting across Cargo.toml, dprint, workflows, configs
- Migrate from Taskfile to justfile for build automation
- Optimize CI workflow triggers with path filters
- Configure rust-analyzer to enable all features

### Fixed

- Empty-line handling in `StdioTransport::read_message()`
- `Retry-After` header support for 429 responses
- Proper URL parsing for `bun://` URIs
- UTF-8 truncation helpers consolidated to `src/util.rs`
- `.gitignore` was excluding `src/bin/` directory
- Deprecated warning in CLI integration tests

### Removed

- Old flat module structure (`src/main.rs`, `src/http.rs`, etc.)
- `tests/` directory (migrated to inline modules)
- TESTING.md and INDEX.md (consolidated into AGENTS.md)
- Taskfile.yml (replaced by justfile)

## [0.3.0] - 2025-11-05

### Added

- CLI search mode with `--search` flag and multiple output formats (JSON, text,
  markdown)
- Dual testing strategy: fast unit tests (mocked) + feature-gated integration
  tests
- Comprehensive error path and retry tests with timing validation
- Path traversal protection and input validation for CLI mode
- Taskfile automation with 50+ tasks for development workflow
- GitHub Actions integration with collapsible output groups
- Empty query validation to prevent API errors
- Bytes written verification for file output operations

### Changed

- Markdown format fetches raw MDX sources instead of formatting search text
- Refactored retry loop to use `usize` consistently (removed 3 clippy
  suppressions)
- Replace live API calls with mocked unit tests for faster CI
- Improved error messages with structured logging for MDX fetch errors
- Enhanced test isolation using `tempfile::Builder` with relative paths
- Switched coverage tooling from tarpaulin to `cargo-llvm-cov`

### Fixed

- Integration workflow now runs `#[ignore]` tests with `--include-ignored` flag
- Rust 2024 edition drop order warnings using `let...else` syntax
- Clippy `manual_let_else` warnings in SSE parsing
- Path validation now rejects absolute paths for security
- Test race conditions from hardcoded temporary filenames

### Removed

- Duplicate `test-unit-only` Taskfile target
- Redundant `.gitignore` patterns (logs, node_modules)
- Manual cleanup code (replaced with RAII via `tempfile`)

## [0.2.1] - 2025-11-04

### Added

- Enhanced CI with lint checks and SHA256 checksums in releases

### Changed

- Updated Cargo.toml version to 0.2.1

## [0.2.0] - 2025-11-04

### Added

- MCP resources support for better client compatibility (#3)
- `resources/list` method returning Bun Documentation resource
- `resources/read` method with URI parsing (e.g., `bun://docs?query=Bun.serve`)
- Comprehensive test suite with 46 tests covering protocol compliance
- `cargo-nextest` configuration for faster test execution
- HTTP edge case tests for SSE parsing, retries, and error handling
- CLI argument tests for `--help` and `--version` flags
- GitHub Actions workflows upgraded to v5/v6
- Codecov integration with cobertura.xml support
- Pre-commit hooks for code quality

### Changed

- Reorganized code into `http`, `protocol`, and `transport` modules
- Improved documentation formatting and test guidance
- Expanded test coverage to 46 tests with mocked HTTP responses

### Fixed

- Clippy `never_loop` warning in `handle_args` function
- CI permissions for GitHub Actions workflows

## [0.1.2] - 2025-11-03

### Fixed

- Resolved clippy `never_loop` warning in argument handling

## [0.1.1] - 2025-11-03

### Added

- `BunDocsClient` HTTP client with Server-Sent Events (SSE) proxy support
- Request forwarding to `bun.com/docs/mcp` with SSE response parsing
- Stdio transport module for stdin/stdout communication
- Basic error handling with JSON-RPC error responses

### Changed

- Updated CI workflow to support new HTTP client functionality
- Improved Makefile with additional development targets
- Enhanced README with architecture details

## [0.1.0] - 2025-11-03

### Added

- CI/CD pipeline using `cargo-zigbuild` for cross-platform builds
- Release workflow for automated binary distribution
- Dependabot configuration for dependency updates
- Claude code review workflow integration
- Increased Dependabot open pull requests limit to 2

### Changed

- Migrated from standard cargo to `cargo-zigbuild` for better cross-compilation

## [0.0.1] - 2025-11-03

### Added

- Initial project structure with Rust MCP proxy skeleton
- JSON-RPC 2.0 protocol types and request/response handling
- Basic stdio transport for reading/writing JSON-RPC messages
- `initialize` and `tools/list` method handlers
- MIT license
- GitHub repository setup

<!--tag-link-definitions-start-->

<!--[rust-legacy]: https://github.com/kjanat/bun-docs-mcp-proxy/compare/X...rust-legacy-->

[Unreleased]: https://github.com/kjanat/bun-docs-mcp-proxy/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/kjanat/bun-docs-mcp-proxy/compare/v0.3.0...v1.0.0
[0.3.0]: https://github.com/kjanat/bun-docs-mcp-proxy/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/kjanat/bun-docs-mcp-proxy/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/kjanat/bun-docs-mcp-proxy/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/kjanat/bun-docs-mcp-proxy/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/kjanat/bun-docs-mcp-proxy/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/kjanat/bun-docs-mcp-proxy/compare/v0.0.1...v0.1.0
[0.0.1]: https://github.com/kjanat/bun-docs-mcp-proxy/releases/tag/v0.0.1

<!--tag-link-definitions-end-->

<!--link-definitions-start-->

[Keep a Changelog]: https://keepachangelog.com/en/1.1.0/
[Semantic Versioning]: https://semver.org/spec/v2.0.0.html

<!--link-definitions-end-->

<!--markdownlint-disable-file no-duplicate-heading-->
