# Contributing to Bun Docs MCP Proxy

Thank you for your interest in contributing! This document provides guidelines and instructions for contributing to the project.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
- [Development Workflow](#development-workflow)
- [Code Standards](#code-standards)
- [Testing](#testing)
- [Pull Request Process](#pull-request-process)
- [Commit Message Guidelines](#commit-message-guidelines)

## Code of Conduct

Please be respectful and constructive in all interactions. We're all here to build something useful together.

## Getting Started

### Prerequisites

- Rust 1.81.0 or later
- Cargo (comes with Rust)
- [Task](https://taskfile.dev) (optional but recommended)
- Git

### Fork and Clone

1. Fork the repository on GitHub
2. Clone your fork locally:
   ```bash
   git clone https://github.com/YOUR_USERNAME/bun-docs-mcp-proxy.git
   cd bun-docs-mcp-proxy
   ```
3. Add upstream remote:
   ```bash
   git remote add upstream https://github.com/kjanat/bun-docs-mcp-proxy.git
   ```

### Build the Project

```bash
# Using Task (recommended)
task br

# Or using cargo directly
cargo build --release
```

## Development Workflow

### Using Task (Recommended)

This project uses [Task](https://taskfile.dev) for build automation:

```bash
# Quick reference
task br          # Build release binary
task t           # Run all tests
task c           # Run all checks (fmt + clippy + tests)
task dev         # Watch mode (auto-rebuild on changes)
task cov         # Generate coverage report

# See all available tasks
task --list-all
```

### Development Cycle

1. **Create a feature branch:**

   ```bash
   git checkout -b feature/your-feature-name
   ```

2. **Make your changes**

3. **Run checks frequently:**

   ```bash
   task c  # Runs format, clippy, and tests
   ```

4. **Watch mode for rapid iteration:**
   ```bash
   task dev  # Auto-rebuilds on file changes
   ```

## Code Standards

### Formatting

Code must be formatted with `rustfmt`:

```bash
# Using Task
task f

# Or using cargo
cargo fmt
```

### Linting

Code must pass clippy lints without warnings:

```bash
# Using Task
task l

# Or using cargo
cargo clippy --all-targets --all-features -- -D warnings
```

### Lint Configuration

The project has specific clippy lint rules in `Cargo.toml`. Key requirements:

- **No `unwrap()` in production code** - Use proper error handling
- **No `expect()` in production code** - Except for tests where it's acceptable
- **Prefer explicit error types** - Use `anyhow` or custom error types
- **Follow existing patterns** - Match the style of surrounding code

### Code Quality Checklist

Before submitting a PR, ensure:

- [ ] Code is formatted (`task f`)
- [ ] Clippy passes with no warnings (`task l`)
- [ ] All tests pass (`task t`)
- [ ] New functionality has tests
- [ ] Documentation is updated
- [ ] No compiler warnings
- [ ] Binary size hasn't increased significantly (check with `ls -lh target/release/bun-docs-mcp-proxy`)

## Testing

### Running Tests

```bash
# All tests
task t

# Unit tests only
task tu

# Integration tests only
task ti

# With coverage
task cov
```

### Writing Tests

- **Unit tests** - Place in the same file as the code, in a `#[cfg(test)]` module
- **Integration tests** - Place in `tests/` directory
- **Mock HTTP responses** - Use `mockito` for testing HTTP client code

### Test Requirements

- All new features must have tests
- Bug fixes should include a regression test
- Aim for >80% code coverage
- Tests should be fast (avoid sleep/delays when possible)

### Manual Testing

Test the proxy manually with example requests:

```bash
# Build in debug mode
cargo build

# Run with debug logging
RUST_LOG=debug ./target/debug/bun-docs-mcp-proxy

# In another terminal, send a test request
echo '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"SearchBun","arguments":{"query":"Bun.serve"}}}' | \
./target/debug/bun-docs-mcp-proxy
```

## Pull Request Process

### Before Submitting

1. **Sync with upstream:**

   ```bash
   git fetch upstream
   git rebase upstream/master
   ```

2. **Run all checks:**

   ```bash
   task c  # format + clippy + tests
   ```

3. **Update documentation:**
   - Update README.md if adding features
   - Update CLAUDE.md if changing architecture
   - Add/update comments for complex code

### Submitting the PR

1. **Push to your fork:**

   ```bash
   git push origin feature/your-feature-name
   ```

2. **Create the PR on GitHub**

3. **Fill out the PR template completely:**
   - Clear description of changes
   - Link to related issues
   - List of changes made
   - Testing performed
   - Breaking changes (if any)

4. **Address review feedback:**
   - Respond to comments
   - Make requested changes
   - Push updates to the same branch

### PR Requirements

- [ ] Follows code standards (formatting, linting)
- [ ] All tests pass
- [ ] Includes tests for new functionality
- [ ] Documentation updated
- [ ] No merge conflicts with master
- [ ] PR template filled out completely
- [ ] Commit messages follow guidelines

## Commit Message Guidelines

### Format

```
<type>(<scope>): <subject>

<body>

<footer>
```

### Types

- **feat**: New feature
- **fix**: Bug fix
- **docs**: Documentation changes
- **style**: Code style changes (formatting, no logic change)
- **refactor**: Code refactoring
- **perf**: Performance improvements
- **test**: Adding or updating tests
- **chore**: Build process, dependencies, tooling

### Examples

```
feat(cli): add --format markdown option for CLI search

Add support for markdown output format in CLI mode.
Fetches raw MDX sources from documentation URLs.

Closes #42
```

```
fix(http): handle empty SSE responses gracefully

Previously would panic on empty response stream.
Now returns appropriate error message.

Fixes #38
```

### Guidelines

- Use present tense ("add feature" not "added feature")
- Use imperative mood ("move cursor to..." not "moves cursor to...")
- First line is 50 characters or less
- Reference issues and PRs in the footer

## Questions?

- **Documentation**: Check [README.md](../README.md) and [CLAUDE.md](../CLAUDE.md)
- **Discussions**: Use [GitHub Discussions](https://github.com/kjanat/bun-docs-mcp-proxy/discussions)
- **Issues**: Search [existing issues](https://github.com/kjanat/bun-docs-mcp-proxy/issues) first

Thank you for contributing! 🎉
