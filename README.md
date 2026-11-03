# Bun Docs MCP

[![CI](https://github.com/kjanat/bun-docs-mcp-proxy/actions/workflows/ci.yml/badge.svg?branch=master)][ci.yml]
[![Release](https://github.com/kjanat/bun-docs-mcp-proxy/actions/workflows/release.yml/badge.svg)][release.yml]
[![codecov](https://codecov.io/gh/kjanat/bun-docs-mcp-proxy/graph/badge.svg?token=ySY6HF2Vbx)][codecov]

Local full-text search for Bun documentation. The binary downloads Bun's static
`llms.txt` and `llms-full.txt` assets, builds an in-memory Tantivy index, and
serves it to Zed, Claude Code, or the standalone CLI. It does not call Bun's
hosted MCP endpoint.

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
bun-docs -s "Bun.serve"                          # JSON with matching excerpts
bun-docs -s "server" -f text --limit 3           # Three compact results
bun-docs -s "WebSocket" -f markdown -o ws.md      # Markdown result summaries
bun-docs -s "server" -f markdown --full -l 1      # Best matching page in full
```

Search returns up to ten results by default. Each result includes a short
passage from the page matching the query, even when the documentation index has
no description. Use `--limit` (`-l`) to choose the number of results and
`--full` to include complete page bodies in any format. JSON keeps the title,
URL, description, and score fields and adds `excerpt`; `--full` also adds
`content`. Successful searches are quiet on stderr unless you set `RUST_LOG` or
save to a file. `--format` requires one of `json`, `text`, or `markdown`.

### MCP Server

Implements
[MCP 2026-07-28](https://modelcontextprotocol.io/specification/2026-07-28) over
stdio. Clients send the protocol version and client capabilities in each
request's `params._meta`. `server/discover` reports the supported version and
server capabilities; `tools/list` and `tools/call` work directly without a
handshake. Discovery and tool-list results include public caching hints with a
five-minute TTL. Every successful result includes server identity metadata.
Malformed JSON returns `-32700`; malformed JSON-RPC requests return `-32600`.
Errors omit the response ID when no valid request ID is available. Older
protocol versions and `initialize` are unsupported.

Add to your `.mcp.json`:

```json
{
  "mcpServers": {
    "bun-docs": {
      "command": "/path/to/bun-docs"
    }
  }
}
```

Or install the [Zed extension][zed-extension] (auto-downloads the binary).

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
