//! MCP method handlers.
//!
//! This module contains all the JSON-RPC method handlers for the MCP protocol,
//! including `initialize`, `tools/list`, and `tools/call`.
//!
//! The proxy transparently forwards requests to the upstream Bun Docs API and
//! relays responses. The only local logic is in `initialize`, which overrides
//! `serverInfo` with the proxy's identity while passing through the upstream's
//! capabilities.

use tracing::{error, info, instrument, warn};

use crate::{
    mcp::{JsonRpcRequest, JsonRpcResponse, MCP_PROTOCOL_VERSION, SERVER_NAME, error_code},
    upstream::{BunDocsClient, UpstreamResponse},
};

// ============================================================================
// Helper Functions
// ============================================================================

/// Attempts to extract the "id" field from a potentially malformed JSON string.
///
/// This is used to provide better error correlation when JSON-RPC parsing fails.
/// Uses a simple regex-based approach to find the id field even in invalid JSON.
///
/// # Arguments
/// * `json_str` - The raw JSON string (may be malformed).
///
/// # Returns
/// The extracted id as a `serde_json::Value`, or `Value::Null` if extraction fails.
pub fn extract_id_from_json(json_str: &str) -> serde_json::Value {
    // Try parsing as valid JSON first (fastest path for valid JSON with wrong structure)
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_str)
        && let Some(id) = parsed.get("id")
    {
        return id.clone();
    }

    // Fallback: simple pattern matching for "id": followed by a value
    // Handles: "id":123, "id":"str", "id":null
    // This regex is intentionally simple - we just want to extract the id for correlation
    if let Some(id_start) = json_str.find("\"id\"") {
        let Some(after_id) = json_str.get(id_start + 4..) else {
            return serde_json::Value::Null;
        };
        // Skip whitespace and colon
        let trimmed = after_id.trim_start().strip_prefix(':').map(str::trim_start);
        if let Some(value_start) = trimmed {
            // Try to parse the value portion
            if let Some(after_quote) = value_start.strip_prefix('"') {
                // String id - find closing quote
                if let Some(end) = after_quote.find('"') {
                    let id_value = after_quote.get(..end).unwrap_or("").to_owned();
                    return serde_json::Value::String(id_value);
                }
            } else if value_start.starts_with("null") {
                return serde_json::Value::Null;
            } else {
                // Numeric id - extract digits
                let num_str: String = value_start
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '-')
                    .collect();
                if let Ok(num) = num_str.parse::<i64>() {
                    return serde_json::json!(num);
                }
            }
        }
    }

    serde_json::Value::Null
}

// ============================================================================
// Upstream Response Conversion
// ============================================================================

/// Converts an upstream forward result into a JSON-RPC response.
///
/// Handles success, upstream JSON-RPC errors, and transport/network errors uniformly.
/// This helper deduplicates the `match UpstreamResponse { Ok/Err }` → `JsonRpcResponse`
/// pattern used across multiple handlers.
fn upstream_to_jsonrpc(
    result: anyhow::Result<UpstreamResponse>,
    request_id: serde_json::Value,
) -> JsonRpcResponse {
    match result {
        Ok(UpstreamResponse::Ok(value)) => JsonRpcResponse::success(request_id, value),
        Ok(UpstreamResponse::Err {
            code,
            message,
            data: err_data,
        }) => {
            // Upstream returned a JSON-RPC error — propagate it.
            // Note: code is i64 from upstream, but JsonRpcError uses i32.
            // Truncation is acceptable for standard JSON-RPC error codes.
            #[expect(
                clippy::cast_possible_truncation,
                reason = "JSON-RPC error codes fit in i32"
            )]
            let code_i32 = code as i32;
            if let Some(extra) = err_data {
                JsonRpcResponse::error_with_data(request_id, code_i32, message, extra)
            } else {
                JsonRpcResponse::error(request_id, code_i32, message)
            }
        }
        Err(e) => {
            error!("Failed to forward request: {}", e);
            JsonRpcResponse::error(
                request_id,
                error_code::INTERNAL_ERROR,
                format!("Internal error: {e}"),
            )
        }
    }
}

// ============================================================================
// MCP Method Handlers
// ============================================================================

/// Handles an `initialize` JSON-RPC request by forwarding it to the upstream
/// Bun Docs API and merging the response with the proxy's own identity.
///
/// The upstream's `capabilities`, `protocolVersion`, and `instructions` are passed
/// through verbatim. The proxy overrides `serverInfo` with its own name and version,
/// since from the stdio client's perspective the proxy IS the server.
///
/// If the upstream is unreachable, falls back to a minimal hardcoded response
/// so the proxy can still start.
///
/// # Arguments
/// * `client` - A reference to the `BunDocsClient` for forwarding the request.
/// * `request` - The incoming `JsonRpcRequest` (params may contain client info).
/// * `request_id` - The request identifier for the response.
///
/// # Returns
/// A `JsonRpcResponse` containing the initialization result.
#[instrument(name = "initialize", skip(client, request))]
pub async fn handle_initialize(
    client: &BunDocsClient,
    request: &JsonRpcRequest,
    request_id: serde_json::Value,
) -> JsonRpcResponse {
    let upstream_request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "initialize",
        "params": request.params
    });

    match client.forward_request(upstream_request).await {
        Ok(UpstreamResponse::Ok(mut result)) => {
            // Override serverInfo with proxy identity — the proxy IS the server
            // from the stdio client's perspective.
            if let Some(obj) = result.as_object_mut() {
                obj.insert(
                    "serverInfo".into(),
                    serde_json::json!({
                        "name": SERVER_NAME,
                        "version": env!("CARGO_PKG_VERSION")
                    }),
                );
            }
            info!("initialize forwarded to upstream");
            JsonRpcResponse::success(request_id, result)
        }
        Ok(UpstreamResponse::Err { code, message, .. }) => {
            warn!("Upstream initialize returned error {code}: {message}, using fallback");
            fallback_initialize(request_id)
        }
        Err(e) => {
            warn!("Upstream initialize failed: {e}, using fallback");
            fallback_initialize(request_id)
        }
    }
}

/// Fallback initialize response when upstream is unreachable.
///
/// Returns a minimal response with only `tools` capability (no resources,
/// since upstream doesn't support them).
#[must_use]
fn fallback_initialize(request_id: serde_json::Value) -> JsonRpcResponse {
    JsonRpcResponse::success(
        request_id,
        serde_json::json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": SERVER_NAME,
                "version": env!("CARGO_PKG_VERSION")
            }
        }),
    )
}

/// Handles a `tools/list` JSON-RPC request by forwarding it to the upstream Bun Docs API.
///
/// The proxy transparently relays the upstream's tool definitions so that clients
/// receive authoritative, up-to-date descriptions and schemas.
///
/// # Arguments
/// * `client` - A reference to the `BunDocsClient` for making the API call.
/// * `request_id` - The request identifier for the response.
///
/// # Returns
/// A `JsonRpcResponse` containing the list of tools from upstream.
#[instrument(name = "tools_list", skip(client))]
pub async fn handle_tools_list(
    client: &BunDocsClient,
    request_id: serde_json::Value,
) -> JsonRpcResponse {
    let upstream_request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "tools/list"
    });

    let result = client.forward_request(upstream_request).await;
    info!("tools/list forwarded to upstream");
    upstream_to_jsonrpc(result, request_id)
}

/// Handles a `tools/call` JSON-RPC request by forwarding it to the Bun Docs API.
///
/// This function takes an incoming `tools/call` request, constructs a new request
/// with the same parameters, and sends it to the Bun Docs API via the `BunDocsClient`.
/// It then processes the response, extracting the `result` field on success.
///
/// # Arguments
/// * `client` - A reference to the `BunDocsClient` for making the API call.
/// * `request` - A reference to the incoming `JsonRpcRequest`.
/// * `request_id` - The request identifier for the response.
///
/// # Returns
/// A `JsonRpcResponse` to be sent back to the client.
#[instrument(name = "tools_call", skip(client, request), fields(tool_name = tracing::field::Empty, query = tracing::field::Empty))]
pub async fn handle_tools_call(
    client: &BunDocsClient,
    request: &JsonRpcRequest,
    request_id: serde_json::Value,
) -> JsonRpcResponse {
    // Extract tool name and query for tracing
    if let Some(params) = &request.params {
        if let Some(name) = params.get("name").and_then(|v| v.as_str()) {
            tracing::Span::current().record("tool_name", name);
        }
        if let Some(query) = params
            .get("arguments")
            .and_then(|a| a.get("query"))
            .and_then(|v| v.as_str())
        {
            tracing::Span::current().record("query", query);
        }
    }

    // Forward entire request to Bun Docs API
    let original_request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": request.method,
        "params": request.params
    });

    let result = client.forward_request(original_request).await;
    info!("tools/call forwarded to upstream");
    upstream_to_jsonrpc(result, request_id)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::expect_used, reason = "tests can use expect()")]
#[allow(clippy::unwrap_used, reason = "tests can use unwrap()")]
#[allow(clippy::indexing_slicing, reason = "tests use array indexing")]
#[allow(clippy::default_numeric_fallback, reason = "test literals")]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{
        mcp::{JsonRpcRequest, MCP_PROTOCOL_VERSION, SERVER_NAME},
        upstream::bun_docs as http,
    };

    #[tokio::test]
    async fn test_handle_initialize_forwarded() {
        // Mock upstream initialize response
        let mut server = mockito::Server::new_async().await;

        let mock = server
            .mock("POST", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"jsonrpc":"2.0","result":{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"upstream-server","version":"0.1.0"}},"id":1}"#)
            .expect(1)
            .create_async()
            .await;

        let client =
            http::BunDocsClient::with_base_url(&server.url()).expect("valid mock server URL");
        let request = JsonRpcRequest {
            jsonrpc: Some("2.0".to_owned()),
            id: Some(json!(1)),
            method: "initialize".to_owned(),
            params: None,
        };

        let response = handle_initialize(&client, &request, json!(1)).await;
        let serialized = serde_json::to_value(&response).unwrap();

        mock.assert_async().await;
        drop(server);

        assert_eq!(serialized["id"], 1);
        // Capabilities and protocolVersion come from upstream
        assert!(serialized["result"]["capabilities"]["tools"].is_object());
        assert_eq!(
            serialized["result"]["protocolVersion"],
            MCP_PROTOCOL_VERSION
        );
        // serverInfo is overridden with proxy identity
        assert_eq!(serialized["result"]["serverInfo"]["name"], SERVER_NAME);
        assert_eq!(
            serialized["result"]["serverInfo"]["version"],
            env!("CARGO_PKG_VERSION")
        );
    }

    #[tokio::test]
    async fn test_handle_initialize_fallback_on_upstream_error() {
        // Mock upstream returning an error
        let mut server = mockito::Server::new_async().await;

        let mock = server
            .mock("POST", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"jsonrpc":"2.0","error":{"code":-32601,"message":"Method not found"},"id":1}"#,
            )
            .expect(1)
            .create_async()
            .await;

        let client =
            http::BunDocsClient::with_base_url(&server.url()).expect("valid mock server URL");
        let request = JsonRpcRequest {
            jsonrpc: Some("2.0".to_owned()),
            id: Some(json!(1)),
            method: "initialize".to_owned(),
            params: None,
        };

        let response = handle_initialize(&client, &request, json!(1)).await;
        let serialized = serde_json::to_value(&response).unwrap();

        mock.assert_async().await;
        drop(server);

        // Falls back to hardcoded response
        assert_eq!(serialized["id"], 1);
        assert_eq!(
            serialized["result"]["protocolVersion"],
            MCP_PROTOCOL_VERSION
        );
        assert_eq!(serialized["result"]["serverInfo"]["name"], SERVER_NAME);
        assert!(serialized["result"]["capabilities"]["tools"].is_object());
        // No resources capability in fallback
        assert!(serialized["result"]["capabilities"]["resources"].is_null());
    }

    #[tokio::test]
    async fn test_handle_tools_list_mocked() {
        // Mock upstream tools/list response
        let mut server = mockito::Server::new_async().await;

        let mock = server
            .mock("POST", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"jsonrpc":"2.0","result":{"tools":[{"name":"SearchBun","description":"Search Bun documentation","inputSchema":{"type":"object","properties":{"query":{"type":"string","description":"Search query"}},"required":["query"]}}]},"id":"test-id"}"#)
            .expect(1)
            .create_async()
            .await;

        let client =
            http::BunDocsClient::with_base_url(&server.url()).expect("valid mock server URL");

        let response = handle_tools_list(&client, json!("test-id")).await;
        let serialized = serde_json::to_value(&response).unwrap();

        mock.assert_async().await;
        drop(server);

        assert_eq!(serialized["id"], "test-id");
        assert!(serialized["result"]["tools"].is_array());

        let tools = serialized["result"]["tools"].as_array().unwrap();
        assert!(!tools.is_empty());
        // Verify structure (not hardcoded values — upstream controls content)
        let tool = &tools[0];
        assert!(tool["name"].is_string());
        assert!(tool["description"].is_string());
        assert!(tool["inputSchema"]["type"].is_string());
        assert_eq!(tool["inputSchema"]["type"], "object");
    }

    #[tokio::test]
    async fn test_handle_tools_call_mocked() {
        // Mock successful API response without network call
        let mut server = mockito::Server::new_async().await;

        // Mock the SSE stream response - must include jsonrpc and id for JsonRpcEnvelope parsing
        let mock = server
            .mock("POST", "/")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body("data: {\"jsonrpc\":\"2.0\",\"result\":{\"content\":[{\"text\":\"Mocked Bun.serve documentation\",\"type\":\"text\"}]},\"id\":1}\n\n")
            .expect(1)
            .create_async()
            .await;

        let client =
            http::BunDocsClient::with_base_url(&server.url()).expect("valid mock server URL");
        let request = JsonRpcRequest {
            jsonrpc: Some("2.0".to_owned()),
            id: Some(json!(1)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "SearchBun",
                "arguments": {
                    "query": "Bun.serve"
                }
            })),
        };

        let response = handle_tools_call(&client, &request, json!(1)).await;
        let serialized = serde_json::to_value(&response).unwrap();

        mock.assert_async().await;
        drop(server);

        // Verify successful response structure
        assert!(serialized["result"].is_object());
        assert!(serialized["result"]["content"].is_array());
        let content = serialized["result"]["content"].as_array().unwrap();
        assert!(!content.is_empty());
        assert_eq!(content[0]["text"], "Mocked Bun.serve documentation");
    }

    #[tokio::test]
    #[cfg(feature = "integration-tests")]
    async fn test_handle_tools_call_real_api() {
        let client = http::BunDocsClient::new();
        let request = JsonRpcRequest {
            jsonrpc: Some("2.0".to_owned()),
            id: Some(json!(1)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "SearchBun",
                "arguments": {
                    "query": "Bun.serve"
                }
            })),
        };

        let response = handle_tools_call(&client, &request, json!(1)).await;
        let serialized = serde_json::to_value(&response).unwrap();

        assert!(serialized["result"].is_object());
        assert!(serialized["result"]["content"].is_array());
    }

    #[tokio::test]
    #[cfg(feature = "integration-tests")]
    async fn test_handle_tools_call_empty_query() {
        // NOTE: This test reflects Bun API's current behavior for empty query.
        // As of now, Bun returns {"content":[{"text":"No results found","type":"text"}],"isError":true}
        // If Bun changes this behavior (e.g., returns docs overview), update expected output accordingly.
        let client = http::BunDocsClient::new();
        let request = JsonRpcRequest {
            jsonrpc: Some("2.0".to_owned()),
            id: Some(json!(2)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "SearchBun",
                "arguments": {
                    "query": ""
                }
            })),
        };

        let response = handle_tools_call(&client, &request, json!(2)).await;
        let serialized = serde_json::to_value(&response).unwrap();

        // Proxy should forward successfully; Bun API decides what empty query means
        assert!(serialized["result"].is_object());
    }

    #[tokio::test]
    async fn test_handle_tools_call_with_network_error() {
        // Test that network errors are properly converted to JSON-RPC error responses
        let mut server = mockito::Server::new_async().await;

        // Mock all requests to fail with 503
        let _mock = server
            .mock("POST", mockito::Matcher::Any)
            .with_status(503_usize)
            .with_body("Service Unavailable")
            .expect_at_least(1_usize)
            .create_async()
            .await;

        let client =
            http::BunDocsClient::with_base_url(&server.url()).expect("valid mock server URL");
        let request = JsonRpcRequest {
            jsonrpc: Some("2.0".to_owned()),
            id: Some(json!(1)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "SearchBun",
                "arguments": {"query": "test"}
            })),
        };

        let response = handle_tools_call(&client, &request, json!(1)).await;
        let serialized = serde_json::to_value(&response).unwrap();

        drop(server);

        // Verify error response structure (after max retries)
        assert!(
            serialized["error"].is_object(),
            "Should have error field in response"
        );
        assert_eq!(
            serialized["error"]["code"], -32_603_i32,
            "Should be internal error code"
        );
        assert!(
            serialized["error"]["message"]
                .as_str()
                .unwrap()
                .contains("error"),
            "Error message should describe the issue"
        );
    }

    // ============================================================================
    // extract_id_from_json tests
    // ============================================================================

    #[test]
    fn test_extract_id_from_valid_json_numeric() {
        let json = r#"{"jsonrpc":"2.0","id":123,"method":"test"}"#;
        let id = extract_id_from_json(json);
        assert_eq!(id, json!(123));
    }

    #[test]
    fn test_extract_id_from_valid_json_string() {
        let json = r#"{"jsonrpc":"2.0","id":"request-1","method":"test"}"#;
        let id = extract_id_from_json(json);
        assert_eq!(id, json!("request-1"));
    }

    #[test]
    fn test_extract_id_from_valid_json_null() {
        let json = r#"{"jsonrpc":"2.0","id":null,"method":"test"}"#;
        let id = extract_id_from_json(json);
        assert_eq!(id, serde_json::Value::Null);
    }

    #[test]
    fn test_extract_id_from_malformed_json_numeric() {
        // Malformed JSON (missing closing brace) but has id
        let json = r#"{"jsonrpc":"2.0","id":456,"method":"test"#;
        let id = extract_id_from_json(json);
        assert_eq!(id, json!(456));
    }

    #[test]
    fn test_extract_id_from_malformed_json_string() {
        // Malformed JSON but has string id
        let json = r#"{"jsonrpc":"2.0","id":"my-id","method":"test"#;
        let id = extract_id_from_json(json);
        assert_eq!(id, json!("my-id"));
    }

    #[test]
    fn test_extract_id_from_json_no_id_field() {
        let json = r#"{"jsonrpc":"2.0","method":"notification"}"#;
        let id = extract_id_from_json(json);
        assert_eq!(id, serde_json::Value::Null);
    }

    #[test]
    fn test_extract_id_from_empty_string() {
        let id = extract_id_from_json("");
        assert_eq!(id, serde_json::Value::Null);
    }

    #[test]
    fn test_extract_id_from_garbage() {
        let id = extract_id_from_json("not json at all");
        assert_eq!(id, serde_json::Value::Null);
    }

    #[test]
    fn test_extract_id_with_whitespace() {
        let json = r#"{"jsonrpc": "2.0", "id" : 789 , "method": "test"}"#;
        let id = extract_id_from_json(json);
        assert_eq!(id, json!(789));
    }

    #[test]
    fn test_extract_id_negative_number() {
        let json = r#"{"jsonrpc":"2.0","id":-42,"method":"test"}"#;
        let id = extract_id_from_json(json);
        assert_eq!(id, json!(-42));
    }

    // ============================================================================
    // MCP response structure tests (migrated from tests/integration_test.rs)
    // ============================================================================

    #[tokio::test]
    async fn tools_list_response_follows_mcp_schema() {
        // Test that tools/list response follows MCP schema when forwarded from upstream
        let mut server = mockito::Server::new_async().await;

        let mock = server
            .mock("POST", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"jsonrpc":"2.0","result":{"tools":[{"name":"SearchBun","description":"Search Bun docs","inputSchema":{"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}}]},"id":1}"#)
            .expect(1)
            .create_async()
            .await;

        let client =
            http::BunDocsClient::with_base_url(&server.url()).expect("valid mock server URL");

        let response = handle_tools_list(&client, json!(1)).await;
        let serialized = serde_json::to_value(&response).unwrap();

        mock.assert_async().await;
        drop(server);

        let tools = serialized["result"]
            .get("tools")
            .expect("tools field exists")
            .as_array()
            .expect("tools is array");
        assert!(
            !tools.is_empty(),
            "upstream should return at least one tool"
        );

        let tool = tools.first().expect("tools array non-empty");
        assert!(tool.get("name").expect("name field exists").is_string());
        assert!(
            tool.get("description")
                .expect("description field exists")
                .is_string()
        );
        assert!(
            tool.get("inputSchema")
                .expect("inputSchema field exists")
                .is_object()
        );
        assert_eq!(
            tool.get("inputSchema")
                .expect("inputSchema exists")
                .get("type")
                .expect("type field exists"),
            "object"
        );
        assert!(
            tool.get("inputSchema")
                .expect("inputSchema exists")
                .get("properties")
                .expect("properties field exists")
                .is_object()
        );
        assert!(
            tool.get("inputSchema")
                .expect("inputSchema exists")
                .get("required")
                .expect("required field exists")
                .is_array()
        );
    }

    #[test]
    fn unsupported_method_error_follows_jsonrpc_spec() {
        // Test that unsupported method errors follow JSON-RPC spec
        use crate::mcp::error_code;

        let error_response = JsonRpcResponse::error(
            json!(1),
            error_code::METHOD_NOT_FOUND,
            "Method not found: unsupported_method".to_owned(),
        );
        let serialized = serde_json::to_value(&error_response).unwrap();

        assert_eq!(
            serialized.get("jsonrpc").expect("jsonrpc field exists"),
            "2.0"
        );
        assert!(
            serialized
                .get("error")
                .expect("error field exists")
                .is_object()
        );
        assert_eq!(
            serialized
                .get("error")
                .expect("error exists")
                .get("code")
                .expect("code field exists"),
            -32_601_i32
        );
        assert!(
            serialized
                .get("error")
                .expect("error exists")
                .get("message")
                .expect("message field exists")
                .as_str()
                .expect("message is string")
                .contains("Method not found")
        );
    }

    #[test]
    fn parse_error_follows_jsonrpc_spec() {
        // Test that parse errors follow JSON-RPC spec
        use crate::mcp::error_code;

        let error_response = JsonRpcResponse::error(
            json!(null),
            error_code::PARSE_ERROR,
            "Parse error: invalid JSON".to_owned(),
        );
        let serialized = serde_json::to_value(&error_response).unwrap();

        assert_eq!(
            serialized.get("jsonrpc").expect("jsonrpc field exists"),
            "2.0"
        );
        assert!(serialized.get("id").expect("id field exists").is_null());
        assert_eq!(
            serialized
                .get("error")
                .expect("error exists")
                .get("code")
                .expect("code field exists"),
            -32_700_i32
        );
        assert!(
            serialized
                .get("error")
                .expect("error exists")
                .get("message")
                .expect("message field exists")
                .as_str()
                .expect("message is string")
                .contains("Parse error")
        );
    }

    #[test]
    fn internal_error_follows_jsonrpc_spec() {
        // Test that internal errors follow JSON-RPC spec
        use crate::mcp::error_code;

        let error_response = JsonRpcResponse::error(
            json!(1),
            error_code::INTERNAL_ERROR,
            "Internal error: failed to process request".to_owned(),
        );
        let serialized = serde_json::to_value(&error_response).unwrap();

        assert_eq!(
            serialized.get("jsonrpc").expect("jsonrpc field exists"),
            "2.0"
        );
        assert_eq!(
            serialized
                .get("error")
                .expect("error exists")
                .get("code")
                .expect("code field exists"),
            -32_603_i32
        );
        assert!(
            serialized
                .get("error")
                .expect("error exists")
                .get("message")
                .expect("message field exists")
                .as_str()
                .expect("message is string")
                .contains("Internal error")
        );
    }
}
