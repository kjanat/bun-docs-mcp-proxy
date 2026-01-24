# Bun Docs MCP Proxy

[![CI](https://github.com/kjanat/bun-docs-mcp-proxy/actions/workflows/ci.yml/badge.svg?branch=master)][ci.yml]
[![Release](https://github.com/kjanat/bun-docs-mcp-proxy/actions/workflows/release.yml/badge.svg)][release.yml]
[![codecov](https://codecov.io/gh/kjanat/bun-docs-mcp-proxy/graph/badge.svg?token=ySY6HF2Vbx)][codecov]

MCP proxy for Bun documentation. Works with Zed, Claude Code, or as a standalone
CLI.

## Install

Download from [Releases][releases] or build from source:

```bash
cargo install --git https://github.com/kjanat/bun-docs-mcp-proxy
```

**Available platforms:** Linux, macOS, Windows (x86_64 + ARM64), plus musl
variants for Linux.

## Usage

### CLI

```bash
bun-docs-mcp-proxy -s "Bun.serve"                       # JSON output
bun-docs-mcp-proxy -s "WebSocket" -f markdown -o ws.md  # Raw MDX
bun-docs-mcp-proxy -s "test" -f text                    # Plain text
```

### MCP Server

Add to your `.mcp.json`:

```json
{
  "mcpServers": {
    "bun-docs": {
      "command": "/path/to/bun-docs-mcp-proxy"
    }
  }
}
```

Or install the [Zed extension][zed-extension] (auto-downloads the proxy).

## Documentation

- [AGENTS.md](AGENTS.md) - Architecture, commands, build info
- [CONTRIBUTING.md](.github/CONTRIBUTING.md) - Development guide
- [CHANGELOG.md](CHANGELOG.md) - Version history
- [SECURITY.md](.github/SECURITY.md) - Security policy

## License

[MIT](./LICENSE)

<!--Link defs-->

[ci.yml]: https://github.com/kjanat/bun-docs-mcp-proxy/actions/workflows/ci.yml
[codecov]: https://codecov.io/gh/kjanat/bun-docs-mcp-proxy
[releases]: https://github.com/kjanat/bun-docs-mcp-proxy/releases
[release.yml]: https://github.com/kjanat/bun-docs-mcp-proxy/actions/workflows/release.yml
[zed-extension]: https://github.com/kjanat/bun-docs-mcp-zed
