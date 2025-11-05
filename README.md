# Bun Docs MCP Proxy

[![CI](https://github.com/kjanat/bun-docs-mcp-proxy/actions/workflows/ci.yml/badge.svg?branch=master)][ci.yml]
[![Release](https://github.com/kjanat/bun-docs-mcp-proxy/actions/workflows/release.yml/badge.svg)][release.yml]
[![codecov](https://codecov.io/gh/kjanat/bun-docs-mcp-proxy/graph/badge.svg?token=ySY6HF2Vbx)][codecov]

Fast, lightweight MCP proxy for Bun documentation search. Search Bun docs from your editor (Zed, Claude Code) or command line.

## What is it?

A native Rust proxy that bridges MCP clients (like Zed or Claude Code) with Bun's documentation server. Get instant access to Bun documentation through MCP tools and resources, or use the built-in CLI for quick searches.

Works as both an MCP server (stdio mode) and standalone CLI tool (search mode).

## Features

- **Tiny & Fast** — 2.7 MB binary, 4ms startup, ~2-5 MB memory
- **Zero Dependencies** — Single native binary, no runtime required
- **Dual Mode** — MCP server for editors + CLI for terminal searches
- **Rich Output** — JSON, Markdown (raw MDX), or plain text formats
- **Production Ready** — ~86% test coverage, comprehensive error handling
- **Cross-Platform** — Linux, macOS, Windows (x86_64 + ARM64)

## Quick Start

### Installation

**Download from [Releases][releases]:**

```bash
# Linux x86_64
curl -L https://github.com/kjanat/bun-docs-mcp-proxy/releases/latest/download/bun-docs-mcp-proxy-linux-x86_64.tar.gz | tar xz

# macOS Apple Silicon
curl -L https://github.com/kjanat/bun-docs-mcp-proxy/releases/latest/download/bun-docs-mcp-proxy-macos-aarch64.tar.gz | tar xz

# Windows x86_64
# Download .zip from releases page
```

**Or build from source:**

```bash
cargo install --git https://github.com/kjanat/bun-docs-mcp-proxy
# or: task br
```

### CLI Search Mode

Search Bun docs from your terminal:

```bash
# Quick search (JSON output)
bun-docs-mcp-proxy --search "Bun.serve"

# Save as markdown (fetches raw MDX sources)
bun-docs-mcp-proxy -s "WebSocket" -f markdown -o websocket-docs.md

# Plain text output
bun-docs-mcp-proxy -s "test" -f text
```

**Output formats:**

- `json` — Structured data (default)
- `markdown` — Raw MDX documentation sources
- `text` — Plain text extraction

### MCP Server Mode

Use with Zed, Claude Code, or any MCP client:

**Zed Extension:**
Install [bun-docs-mcp-zed][zed-extension] extension (auto-downloads proxy)

**Manual MCP Configuration:**

> `.mcp.json`
>
> ```json
> {
>   "mcpServers": {
>     "bun-docs": {
>       "command": "/path/to/bun-docs-mcp-proxy"
>     }
>   }
> }
> ```

**Available MCP methods:**

- `tools/call` with `SearchBun` — Search documentation
- `resources/read` with `bun://docs?query=...` — Read by URI
- `tools/list`, `resources/list`, `initialize` — Standard MCP

## Documentation

- **[INDEX.md](INDEX.md)** — Complete project navigation and reference
- **[CONTRIBUTING.md](.github/CONTRIBUTING.md)** — Development setup and workflow
- **[TESTING.md](.github/TESTING.md)** — Testing strategy and commands
- **[CHANGELOG.md](CHANGELOG.md)** — Version history and releases
- **[SECURITY.md](.github/SECURITY.md)** — Security policy

### Quick Reference

```bash
# Development
task c          # Run all checks (fmt + clippy + tests)
task dev        # Watch mode (auto-rebuild)
task ci         # Run CI checks locally

# Testing
task t          # Unit tests
task tio        # Integration tests
task cov        # Coverage report

# Build
task br         # Release build
task build-all  # All platforms
```

See [INDEX.md](INDEX.md) for comprehensive command reference, architecture details, and cross-platform builds.

## Performance

**95% smaller** and **25-50x faster** than TypeScript alternatives:

| Metric       | This (Rust) | TypeScript (Bun) |
| ------------ | ----------- | ---------------- |
| Binary Size  | 2.7 MB      | ~50 MB (runtime) |
| Startup      | 4 ms        | ~4ms/~100-200 ms |
| Memory       | ~2-5 MB     | ~30-50 MB        |
| Dependencies | None        | Bun/Node.js      |

## Contributing

Contributions welcome! See [CONTRIBUTING.md](.github/CONTRIBUTING.md) for:

- Development setup
- Code style and quality standards
- Testing requirements
- PR submission process

## License

[MIT](./LICENSE)

---

**Links:**

- [GitHub Repository][repo]
- [Latest Release][releases]
- [Zed Extension][zed-extension]
- [Issue Tracker][issues]

<!--Link defs-->

[ci.yml]: https://github.com/kjanat/bun-docs-mcp-proxy/actions/workflows/ci.yml
[codecov]: https://codecov.io/gh/kjanat/bun-docs-mcp-proxy
[releases]: https://github.com/kjanat/bun-docs-mcp-proxy/releases
[release.yml]: https://github.com/kjanat/bun-docs-mcp-proxy/actions/workflows/release.yml
[repo]: https://github.com/kjanat/bun-docs-mcp-proxy
[zed-extension]: https://github.com/kjanat/bun-docs-mcp-zed
[issues]: https://github.com/kjanat/bun-docs-mcp-proxy/issues
