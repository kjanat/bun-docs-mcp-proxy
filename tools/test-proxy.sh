#!/usr/bin/env bash
# Test script for the local Bun documentation MCP server
set -e

# Get script directory and project root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

PROXY="$PROJECT_ROOT/target/release/bun-docs"

echo "=== Bun Docs MCP Test Suite ==="
echo ""

# Build if needed
if [ ! -f "$PROXY" ]; then
	echo "Building proxy..."
	(cd "$PROJECT_ROOT" && cargo build --release)
	echo ""
fi

# Test 1: tools/list
echo "[Test 1] Testing tools/list method..."
RESULT=$(echo '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}' | $PROXY 2>/dev/null)
if echo "$RESULT" | jq -e '.result.tools[0].name == "search_bun"' >/dev/null; then
	echo "✅ tools/list works"
else
	echo "✗ tools/list failed"
	echo "$RESULT" | jq '.'
	exit 1
fi
echo ""

# Test 2: server discovery
echo "[Test 2] Testing server/discover method..."
RESULT=$(echo '{"jsonrpc":"2.0","id":2,"method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}' | $PROXY 2>/dev/null)
if echo "$RESULT" | jq -e '.result._meta["io.modelcontextprotocol/serverInfo"].name == "bun-docs" and .result.supportedVersions == ["2026-07-28"]' >/dev/null; then
	echo "✅ server/discover works"
else
	echo "✗ server/discover failed"
	echo "$RESULT" | jq '.'
	exit 1
fi
echo ""

# Test 3: tools/call with local search_bun
echo "[Test 3] Testing tools/call with search_bun..."
echo "  Query: 'WebSocket'"
RESULT=$(echo '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"search_bun","arguments":{"query":"WebSocket"},"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}' | timeout 30s $PROXY 2>/dev/null)
if echo "$RESULT" | jq -e '.result.content | length > 0' >/dev/null; then
	COUNT=$(echo "$RESULT" | jq '.result.content | length')
	echo "✅ tools/call works (returned $COUNT results)"
else
	echo "✗ tools/call failed"
	echo "$RESULT" | jq '.'
	exit 1
fi
echo ""

# Test 4: Invalid JSON
echo "[Test 4] Testing error handling (invalid JSON)..."
RESULT=$(echo 'invalid json' | $PROXY 2>/dev/null)
if echo "$RESULT" | jq -e '.error.code == -32700' >/dev/null; then
	echo "✅ Error handling works (Parse error)"
else
	echo "✗ Error handling unexpected"
	echo "$RESULT" | jq '.'
	exit 1
fi
echo ""

# Test 5: Unknown method
echo "[Test 5] Testing unknown method..."
RESULT=$(echo '{"jsonrpc":"2.0","id":5,"method":"unknown_method","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}' | $PROXY 2>/dev/null)
if echo "$RESULT" | jq -e '.error.code == -32601' >/dev/null; then
	echo "✅ Unknown method handled correctly"
else
	echo "✗ Unknown method handling unexpected"
	echo "$RESULT" | jq '.'
	exit 1
fi
echo ""

echo "=== All Tests Passed! ==="
echo ""

# Performance metrics
echo "=== Performance Metrics ==="
echo ""

echo "Binary size:"
if [ -f "$PROXY" ]; then
	# Get size in bytes using stat (GNU and BSD variants), then pretty-print with numfmt if available
	if SIZE_BYTES=$(stat -c %s -- "$PROXY" 2>/dev/null); then
		:
	else
		SIZE_BYTES=$(stat -f %z -- "$PROXY" 2>/dev/null || echo 0)
	fi
	if command -v numfmt >/dev/null 2>&1; then
		echo "  $(numfmt --to=iec "$SIZE_BYTES")"
	else
		echo "  ${SIZE_BYTES}B"
	fi
else
	echo "  (not built)"
fi

echo ""
echo "Startup time (tools/list):"
{ time echo '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}' | $PROXY >/dev/null 2>&1; } 2>&1 | grep real | awk '{print "  " $2}'

echo ""
echo "Full request time (tools/call):"
{ time echo '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"search_bun","arguments":{"query":"test"},"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}' | $PROXY >/dev/null 2>&1; } 2>&1 | grep real | awk '{print "  " $2}'

echo ""
echo "✅ Local MCP server is production-ready!"
