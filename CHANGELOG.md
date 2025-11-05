# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0] - 2025-11-05

### Added

- CLI search mode with `--search` flag and multiple output formats (JSON, text, markdown)
- Dual testing strategy: fast unit tests (mocked) + feature-gated integration tests
- Comprehensive error path and retry tests with timing validation
- Path traversal protection and input validation for CLI mode
- Taskfile automation with 50+ tasks for development workflow
- GitHub Actions integration with collapsible output groups
- Empty query validation to prevent API errors
- Bytes written verification for file output operations

### Changed

- Markdown format fetches raw MDX sources instead of formatting search text
- Refactored retry loop to use `usize` consistently (removed 3 clippy suppressions)
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

[unreleased]: https://github.com/kjanat/bun-docs-mcp-proxy/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/kjanat/bun-docs-mcp-proxy/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/kjanat/bun-docs-mcp-proxy/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/kjanat/bun-docs-mcp-proxy/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/kjanat/bun-docs-mcp-proxy/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/kjanat/bun-docs-mcp-proxy/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/kjanat/bun-docs-mcp-proxy/compare/v0.0.1...v0.1.0
[0.0.1]: https://github.com/kjanat/bun-docs-mcp-proxy/releases/tag/v0.0.1
