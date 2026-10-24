//! Locally implemented MCP method handlers.

use tracing::error;

use crate::{
    docs::{DEFAULT_SEARCH_LIMIT, DocsCorpus},
    mcp::{JsonRpcRequest, JsonRpcResponse, MCP_PROTOCOL_VERSION, SERVER_NAME, error_code},
    util::truncate_utf8,
};

// The discovery information and tool definitions are static and public.
const METADATA_TTL_MS: u64 = 300_000;

fn server_metadata() -> serde_json::Value {
    serde_json::json!({
        "io.modelcontextprotocol/serverInfo": {
            "name": SERVER_NAME,
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}

/// Reports the server's supported protocol revision, capabilities, and identity.
#[must_use]
pub fn handle_discover(request_id: serde_json::Value) -> JsonRpcResponse {
    JsonRpcResponse::success(
        request_id,
        serde_json::json!({
            "resultType": "complete",
            "ttlMs": METADATA_TTL_MS,
            "cacheScope": "public",
            "supportedVersions": [MCP_PROTOCOL_VERSION],
            "capabilities": {
                "tools": {}
            },
            "_meta": server_metadata()
        }),
    )
}

/// Returns the tools implemented over the local documentation corpus.
#[must_use]
pub fn handle_tools_list(request_id: serde_json::Value) -> JsonRpcResponse {
    JsonRpcResponse::success(
        request_id,
        serde_json::json!({
            "resultType": "complete",
            "ttlMs": METADATA_TTL_MS,
            "cacheScope": "public",
            "_meta": server_metadata(),
            "tools": [{
                "name": "search_bun",
                "title": "Search bun.com",
                "description": "Search Bun documentation using the local full-text index.",
                "inputSchema": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search terms. Exact API names work well."
                        },
                        "limit": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 25,
                            "description": "Maximum results (default 10)."
                        }
                    },
                    "required": ["query"]
                },
                "annotations": {
                    "destructiveHint": false,
                    "idempotentHint": true,
                    "openWorldHint": false,
                    "readOnlyHint": true
                }
            }]
        }),
    )
}

/// Executes a documentation tool against the local index.
#[must_use]
pub fn handle_tools_call(
    corpus: &DocsCorpus,
    request: &JsonRpcRequest,
    request_id: serde_json::Value,
) -> JsonRpcResponse {
    let Some(params) = request.params.as_ref() else {
        return invalid_params(request_id, "tools/call requires params");
    };
    let Some(name) = params.get("name").and_then(|value| value.as_str()) else {
        return invalid_params(request_id, "tools/call requires a tool name");
    };
    if name != "search_bun" {
        return invalid_params(request_id, format!("Tool {name} not found"));
    }

    let Some(arguments) = params
        .get("arguments")
        .and_then(serde_json::Value::as_object)
    else {
        return invalid_params(request_id, "search_bun requires arguments");
    };
    if arguments.keys().any(|key| key != "query" && key != "limit") {
        return invalid_params(
            request_id,
            "search_bun accepts only query and limit arguments",
        );
    }
    let Some(query) = arguments.get("query").and_then(|value| value.as_str()) else {
        return invalid_params(request_id, "search_bun requires a string query");
    };
    if query.trim().is_empty() {
        return invalid_params(request_id, "search_bun query cannot be empty");
    }

    let limit = match arguments.get("limit") {
        None => DEFAULT_SEARCH_LIMIT,
        Some(limit_json) => match limit_json
            .as_u64()
            .and_then(|limit_u64| usize::try_from(limit_u64).ok())
        {
            Some(limit @ 1..=25) => limit,
            _ => return invalid_params(request_id, "search_bun limit must be between 1 and 25"),
        },
    };

    let hits = match corpus.search(query, limit) {
        Ok(hits) => hits,
        Err(error) => {
            error!("Local documentation search failed: {error}");
            return JsonRpcResponse::error(
                request_id,
                error_code::INTERNAL_ERROR,
                format!("Search failed: {error}"),
            );
        }
    };

    let content: Vec<serde_json::Value> = hits
        .iter()
        .map(|hit| {
            let excerpt = corpus
                .page(&hit.url)
                .map_or("", |page| truncate_utf8(page.body.trim(), 1_200));
            serde_json::json!({
                "type": "text",
                "text": format!(
                    "Title: {}\nLink: {}\nContent: {}",
                    hit.title,
                    hit.url,
                    excerpt
                )
            })
        })
        .collect();

    JsonRpcResponse::success(
        request_id,
        serde_json::json!({ "resultType": "complete", "content": content, "_meta": server_metadata() }),
    )
}

fn invalid_params(request_id: serde_json::Value, message: impl Into<String>) -> JsonRpcResponse {
    JsonRpcResponse::error(request_id, error_code::INVALID_PARAMS, message.into())
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::default_numeric_fallback,
        clippy::expect_used,
        reason = "test code"
    )]

    use serde_json::json;

    use super::*;

    fn corpus() -> DocsCorpus {
        DocsCorpus::from_sources(
            "- [Server](https://bun.com/docs/runtime/http/server.md): HTTP server.\n",
            "# Server\nSource: https://bun.com/docs/runtime/http/server\n\nUse Bun.serve.\n",
        )
        .expect("corpus builds")
    }

    #[test]
    fn discovery_advertises_current_protocol() {
        let response = handle_discover(json!(1));
        assert_eq!(
            response
                .result
                .as_ref()
                .and_then(|result| result.pointer("/supportedVersions/0")),
            Some(&json!(MCP_PROTOCOL_VERSION))
        );
    }

    #[test]
    fn tools_list_advertises_local_search() {
        let response = handle_tools_list(json!(1));
        assert_eq!(
            response
                .result
                .as_ref()
                .and_then(|result| result.pointer("/tools/0/name")),
            Some(&json!("search_bun"))
        );
    }

    #[test]
    fn tools_call_searches_local_corpus() {
        let request: JsonRpcRequest = serde_json::from_value(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "search_bun",
                "arguments": { "query": "server", "limit": 1 }
            }
        }))
        .expect("valid request");

        let response = handle_tools_call(&corpus(), &request, json!(1));
        let text = response
            .result
            .as_ref()
            .and_then(|result| result.pointer("/content/0/text"))
            .and_then(serde_json::Value::as_str)
            .expect("text result");
        assert!(text.contains("Title: Server"));
        assert!(text.contains("Use Bun.serve."));
    }

    #[test]
    fn tools_call_rejects_unknown_tool() {
        let request: JsonRpcRequest = serde_json::from_value(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "unknown_tool",
                "arguments": { "query": "server" }
            }
        }))
        .expect("valid request");

        let response = handle_tools_call(&corpus(), &request, json!(1));
        assert_eq!(
            response.error.as_ref().map(|error| error.code),
            Some(error_code::INVALID_PARAMS)
        );
    }
}
