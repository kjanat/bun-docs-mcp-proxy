# bun-docs-mcp-proxy

Native Rust proxy for Bun documentation MCP context server. Bridges stdio-based MCP clients with the Bun HTTP MCP server.

This npm package provides pre-built binaries for easy installation in Node.js/npm environments.

## Installation

```bash
npm install -g bun-docs-mcp-proxy
# or
npx bun-docs-mcp-proxy
```

## Usage

```bash
bun-docs-mcp-proxy
```

The proxy reads JSON-RPC 2.0 messages from stdin and writes responses to stdout.

## Features

- Zero runtime dependencies (native binary)
- Tiny binary (~2.7 MB with TLS support)
- Fast startup (4ms cold start)
- Low memory (~2-5 MB RSS)
- Cross-platform (Linux, macOS, Windows on x64 and ARM64)

## Supported Platforms

- Linux x64 (glibc)
- Linux ARM64 (glibc)
- macOS x64 (Intel)
- macOS ARM64 (Apple Silicon)
- Windows x64
- Windows ARM64

## Alternative Installation Methods

### From source (requires Rust)
```bash
cargo install bun-docs-mcp-proxy
```

### Using cargo-binstall (faster, no compilation)
```bash
cargo binstall bun-docs-mcp-proxy
```

### Arch Linux (AUR)
```bash
yay -S bun-docs-mcp-proxy
```

## Repository

For source code, issues, and more information, visit:
https://github.com/kjanat/bun-docs-mcp-proxy

## License

MIT
