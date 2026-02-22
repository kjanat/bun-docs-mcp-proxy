//! MCP method handlers.
//!
//! This module contains all the JSON-RPC method handlers for the MCP protocol,
//! including `initialize`, `tools/list`, `tools/call`, `resources/list`, and `resources/read`.

use tracing::{error, info, instrument};

use crate::{
    mcp::{
        BUN_URI_HOST, BUN_URI_SCHEME, JsonRpcRequest, JsonRpcResponse, MCP_PROTOCOL_VERSION,
        SERVER_NAME, content_type, error_code,
    },
    upstream::{BunDocsClient, UpstreamResponse},
};

// ============================================================================
// Helper Functions
// ============================================================================

/// Extracts a required string parameter from a `serde_json::Value` representing JSON-RPC parameters.
///
/// This helper function safely retrieves a string value associated with a given key
/// from a JSON object. It returns an error if the key is missing, or if the value
/// is not a string.
///
/// # Arguments
/// * `params` - A reference to the `serde_json::Value` (expected to be an object)
///   containing the parameters.
/// * `key` - The name of the string parameter to extract.
///
/// # Returns
/// A `Result` which on success contains a string slice (`&str`) of the parameter's value.
/// On failure, it returns a `String` describing the error.
fn get_string_param<'value>(
    params: &'value serde_json::Value,
    key: &str,
) -> Result<&'value str, String> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("Missing or invalid {key} parameter"))
}

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

/// Parses a Bun documentation URI (e.g., `bun://docs?query=example`) and extracts the search query.
///
/// Uses proper URL parsing with percent-decoding support.
/// Accepts `bun://docs` scheme with optional `query` parameter.
///
/// # Arguments
/// * `uri` - The URI string to parse.
///
/// # Returns
/// A `Result` which on success contains the extracted (percent-decoded) search query as a `String`.
/// On failure, it returns a `String` describing the invalid URI format.
fn parse_bun_docs_uri(uri: &str) -> Result<String, String> {
    let parsed =
        reqwest::Url::parse(uri).map_err(|e| format!("Invalid URI format: {uri} ({e})"))?;

    if parsed.scheme() != BUN_URI_SCHEME || parsed.host_str() != Some(BUN_URI_HOST) {
        return Err(format!(
            "Invalid URI format: expected {BUN_URI_SCHEME}://{BUN_URI_HOST}, got {uri}"
        ));
    }

    // Extract query parameter (percent-decoded automatically)
    let query = parsed
        .query_pairs()
        .find(|(k, _)| k == "query")
        .map(|(_, v)| v.into_owned())
        .unwrap_or_default();

    Ok(query)
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

/// Handles an `initialize` JSON-RPC request by returning the protocol version,
/// capabilities, and server information.
///
/// # Arguments
/// * `request_id` - The request identifier for the response.
///
/// # Returns
/// A `JsonRpcResponse` containing the initialization result.
#[must_use]
pub fn handle_initialize(request_id: serde_json::Value) -> JsonRpcResponse {
    // Handle MCP initialize request
    let init_result = serde_json::json!({
        "protocolVersion": MCP_PROTOCOL_VERSION,
        "capabilities": {
            "tools": {},
            "resources": {}
        },
        "serverInfo": {
            "name": SERVER_NAME,
            "version": env!("CARGO_PKG_VERSION")
        }
    });

    JsonRpcResponse::success(request_id, init_result)
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

/// Handles a `resources/list` JSON-RPC request by returning a static list of available resources.
///
/// Currently, this returns a single resource: `bun://docs`.
///
/// # Arguments
/// * `request_id` - The request identifier for the response.
///
/// # Returns
/// A `JsonRpcResponse` containing the list of resources.
#[must_use]
pub fn handle_resources_list(request_id: serde_json::Value) -> JsonRpcResponse {
    // Return available resources
    let resources = serde_json::json!({
        "resources": [{
            "uri": format!("{BUN_URI_SCHEME}://{BUN_URI_HOST}"),
            "name": "Bun Documentation",
            "description": "Search and browse Bun documentation",
            "mimeType": content_type::JSON
        }]
    });

    JsonRpcResponse::success(request_id, resources)
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

/// Handles a `resources/read` JSON-RPC request.
///
/// This function parses the `uri` from the request parameters, extracts a search query
/// from it, and then internally forwards the request as a `tools/call` to the
/// `SearchBun` tool. The result from the API is then wrapped in the MCP resource format.
///
/// # Arguments
/// * `client` - A reference to the `BunDocsClient` for making the API call.
/// * `request` - A reference to the incoming `JsonRpcRequest`.
/// * `request_id` - The request identifier for the response.
///
/// # Returns
/// A `JsonRpcResponse` containing the resource content or an error.
#[instrument(name = "resources_read", skip(client, request), fields(uri = tracing::field::Empty))]
pub async fn handle_resources_read(
    client: &BunDocsClient,
    request: &JsonRpcRequest,
    request_id: serde_json::Value,
) -> JsonRpcResponse {
    // Extract and validate params
    let Some(params) = &request.params else {
        return JsonRpcResponse::error(
            request_id,
            error_code::INVALID_PARAMS,
            "Missing params".to_owned(),
        );
    };

    // Extract URI parameter
    let uri = match get_string_param(params, "uri") {
        Ok(u) => {
            tracing::Span::current().record("uri", u);
            u
        }
        Err(msg) => {
            return JsonRpcResponse::error(request_id, error_code::INVALID_PARAMS, msg);
        }
    };

    // Parse URI to extract query
    let query = match parse_bun_docs_uri(uri) {
        Ok(q) => q,
        Err(msg) => {
            return JsonRpcResponse::error(request_id, error_code::INVALID_PARAMS, msg);
        }
    };

    // Forward to tools/call internally
    let search_request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "tools/call",
        "params": {
            "name": "SearchBun",
            "arguments": {
                "query": query
            }
        }
    });

    match client.forward_request(search_request).await {
        Ok(upstream) => {
            info!("Successfully got resource from Bun Docs");
            match upstream {
                UpstreamResponse::Ok(result) => {
                    // Serialize the result to JSON string for resource text field
                    let text = match serde_json::to_string(&result) {
                        Ok(s) => s,
                        Err(e) => {
                            error!("Failed to serialize resource content: {}", e);
                            return JsonRpcResponse::error(
                                request_id,
                                error_code::INTERNAL_ERROR,
                                format!("Failed to serialize resource: {e}"),
                            );
                        }
                    };

                    // Wrap in MCP resource format
                    let resource_response = serde_json::json!({
                        "contents": [{
                            "uri": uri,
                            "mimeType": content_type::JSON,
                            "text": text
                        }]
                    });

                    JsonRpcResponse::success(request_id, resource_response)
                }
                UpstreamResponse::Err {
                    code,
                    message,
                    data: err_data,
                } => {
                    // Upstream returned a JSON-RPC error - propagate it
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
            }
        }
        Err(e) => {
            error!("Failed to read resource: {}", e);
            JsonRpcResponse::error(
                request_id,
                error_code::INTERNAL_ERROR,
                format!("Internal error: {e}"),
            )
        }
    }
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

    #[test]
    fn test_handle_initialize() {
        let response = handle_initialize(json!(1));
        let serialized = serde_json::to_value(&response).unwrap();

        assert_eq!(serialized["id"], 1);
        assert_eq!(
            serialized["result"]["protocolVersion"],
            MCP_PROTOCOL_VERSION
        );
        assert_eq!(serialized["result"]["serverInfo"]["name"], SERVER_NAME);
        assert!(serialized["result"]["capabilities"]["tools"].is_object());
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

    #[test]
    fn test_initialize_response_version() {
        let response = handle_initialize(json!(1));
        let serialized = serde_json::to_value(&response).unwrap();

        // Verify protocol version matches MCP spec
        assert_eq!(
            serialized["result"]["protocolVersion"],
            MCP_PROTOCOL_VERSION
        );
        // Verify both capabilities are present
        assert!(serialized["result"]["capabilities"]["tools"].is_object());
        assert!(serialized["result"]["capabilities"]["resources"].is_object());
    }

    #[test]
    fn test_handle_resources_list() {
        let response = handle_resources_list(json!("res-list"));
        let serialized = serde_json::to_value(&response).unwrap();

        assert_eq!(serialized["id"], "res-list");
        assert!(serialized["result"]["resources"].is_array());

        let resources = serialized["result"]["resources"].as_array().unwrap();
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0]["uri"], "bun://docs");
        assert_eq!(resources[0]["name"], "Bun Documentation");
        assert_eq!(resources[0]["mimeType"], "application/json");
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
    async fn test_handle_resources_read_mocked() {
        // Mock successful resource read without network call
        let mut server = mockito::Server::new_async().await;

        // Mock the SSE stream response for resource read - must include jsonrpc and id for JsonRpcEnvelope parsing
        let mock = server
            .mock("POST", "/")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body("data: {\"jsonrpc\":\"2.0\",\"result\":{\"content\":[{\"text\":\"Mocked HTTP documentation\",\"type\":\"text\"}]},\"id\":\"res-mock\"}\n\n")
            .expect(1)
            .create_async()
            .await;

        let client =
            http::BunDocsClient::with_base_url(&server.url()).expect("valid mock server URL");
        let request = JsonRpcRequest {
            jsonrpc: Some("2.0".to_owned()),
            id: Some(json!("res-mock")),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "bun://docs?query=HTTP"})),
        };

        let response = handle_resources_read(&client, &request, json!("res-mock")).await;
        let serialized = serde_json::to_value(&response).unwrap();

        mock.assert_async().await;
        drop(server);

        // Verify successful resource response structure
        assert!(serialized["result"]["contents"].is_array());
        let contents = serialized["result"]["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0]["uri"], "bun://docs?query=HTTP");
        assert_eq!(contents[0]["mimeType"], "application/json");
        assert!(contents[0]["text"].is_string());

        // Verify the text contains the mocked result
        let text_content = contents[0]["text"].as_str().unwrap();
        assert!(text_content.contains("Mocked HTTP documentation"));
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
    #[cfg(feature = "integration-tests")]
    async fn test_handle_resources_read_with_query() {
        let client = http::BunDocsClient::new();
        let request = JsonRpcRequest {
            jsonrpc: Some("2.0".to_owned()),
            id: Some(json!("res1")),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "bun://docs?query=Bun.serve"})),
        };

        let response = handle_resources_read(&client, &request, json!("res1")).await;
        let serialized = serde_json::to_value(&response).unwrap();

        assert!(serialized["result"]["contents"].is_array());
        let contents = serialized["result"]["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0]["uri"], "bun://docs?query=Bun.serve");
        assert_eq!(contents[0]["mimeType"], "application/json");
    }

    #[tokio::test]
    #[cfg(feature = "integration-tests")]
    async fn test_handle_resources_read_empty_query() {
        // NOTE: Tests bun://docs (no query param) which proxy converts to empty query string.
        // Bun API currently returns "No results found" for empty queries.
        // If Bun changes to return overview/help for empty query, this test still passes (valid contents array).
        let client = http::BunDocsClient::new();
        let request = JsonRpcRequest {
            jsonrpc: Some("2.0".to_owned()),
            id: Some(json!("res2")),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "bun://docs"})),
        };

        let response = handle_resources_read(&client, &request, json!("res2")).await;
        let serialized = serde_json::to_value(&response).unwrap();

        assert!(serialized["result"]["contents"].is_array());
    }

    #[tokio::test]
    async fn test_handle_resources_read_missing_params() {
        let client = http::BunDocsClient::new();
        let request = JsonRpcRequest {
            jsonrpc: Some("2.0".to_owned()),
            id: Some(json!("res3")),
            method: "resources/read".to_owned(),
            params: None,
        };

        let response = handle_resources_read(&client, &request, json!("res3")).await;
        let serialized = serde_json::to_value(&response).unwrap();

        assert!(serialized["error"].is_object());
        assert_eq!(serialized["error"]["code"], -32602);
        assert!(
            serialized["error"]["message"]
                .as_str()
                .unwrap()
                .contains("Missing params")
        );
    }

    #[tokio::test]
    async fn test_handle_resources_read_invalid_uri() {
        let client = http::BunDocsClient::new();
        let request = JsonRpcRequest {
            jsonrpc: Some("2.0".to_owned()),
            id: Some(json!("res4")),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "invalid://uri"})),
        };

        let response = handle_resources_read(&client, &request, json!("res4")).await;
        let serialized = serde_json::to_value(&response).unwrap();

        assert!(serialized["error"].is_object());
        assert_eq!(serialized["error"]["code"], -32602);
        assert!(
            serialized["error"]["message"]
                .as_str()
                .unwrap()
                .contains("Invalid URI format")
        );
    }

    #[tokio::test]
    async fn test_handle_resources_read_missing_uri_param() {
        let client = http::BunDocsClient::new();
        let request = JsonRpcRequest {
            jsonrpc: Some("2.0".to_owned()),
            id: Some(json!("res5")),
            method: "resources/read".to_owned(),
            params: Some(json!({"other": "value"})),
        };

        let response = handle_resources_read(&client, &request, json!("res5")).await;
        let serialized = serde_json::to_value(&response).unwrap();

        assert!(serialized["error"].is_object());
        assert_eq!(serialized["error"]["code"], -32602);
        assert!(
            serialized["error"]["message"]
                .as_str()
                .unwrap()
                .contains("Missing or invalid uri parameter")
        );
    }

    #[tokio::test]
    #[cfg(feature = "integration-tests")]
    async fn test_handle_resources_read_with_real_search() {
        let client = http::BunDocsClient::new();
        let request = JsonRpcRequest {
            jsonrpc: Some("2.0".to_owned()),
            id: Some(json!("res6")),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "bun://docs?query=HTTP"})),
        };

        let response = handle_resources_read(&client, &request, json!("res6")).await;
        let serialized = serde_json::to_value(&response).unwrap();

        // Real API should return valid results
        assert!(serialized["result"]["contents"].is_array());
        let contents = serialized["result"]["contents"].as_array().unwrap();
        assert!(!contents.is_empty());
    }

    #[test]
    fn test_get_string_param() {
        let params = json!({"uri": "bun://docs", "other": 123});

        assert_eq!(get_string_param(&params, "uri").unwrap(), "bun://docs");
        get_string_param(&params, "other").unwrap_err();
        get_string_param(&params, "missing").unwrap_err();
    }

    #[test]
    fn test_parse_bun_docs_uri() {
        assert_eq!(parse_bun_docs_uri("bun://docs").unwrap(), "");
        assert_eq!(parse_bun_docs_uri("bun://docs?query=test").unwrap(), "test");
        assert_eq!(
            parse_bun_docs_uri("bun://docs?query=Bun.serve").unwrap(),
            "Bun.serve"
        );
        parse_bun_docs_uri("invalid://uri").unwrap_err();
        parse_bun_docs_uri("").unwrap_err();
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

    #[test]
    fn initialize_response_has_required_mcp_fields() {
        // Test that initialize response has required MCP fields
        let response = handle_initialize(json!(1));
        let serialized = serde_json::to_value(&response).unwrap();
        let init_result = &serialized["result"];

        assert!(init_result["protocolVersion"].is_string());
        assert!(init_result["capabilities"].is_object());
        assert!(init_result["serverInfo"]["name"].is_string());
        assert!(init_result["serverInfo"]["version"].is_string());
    }

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
    fn resources_list_response_follows_mcp_schema() {
        // Test that resources/list response follows MCP schema
        let response = handle_resources_list(json!(1));
        let serialized = serde_json::to_value(&response).unwrap();

        let resources = serialized["result"]
            .get("resources")
            .expect("resources field exists")
            .as_array()
            .expect("resources is array");
        assert_eq!(resources.len(), 1_usize);

        let resource = resources.first().expect("resources array non-empty");
        assert_eq!(resource.get("uri").expect("uri field exists"), "bun://docs");
        assert!(resource.get("name").expect("name field exists").is_string());
        assert!(
            resource
                .get("description")
                .expect("description field exists")
                .is_string()
        );
        assert_eq!(
            resource.get("mimeType").expect("mimeType field exists"),
            "application/json"
        );
    }

    #[test]
    fn resources_read_response_structure_valid() {
        // Test that resources/read response follows MCP schema
        let read_response = json!({
            "contents": [{
                "uri": "bun://docs?query=test",
                "mimeType": "application/json",
                "text": "{\"result\": {}}"
            }]
        });

        let contents = read_response
            .get("contents")
            .expect("contents field exists")
            .as_array()
            .expect("contents is array");
        assert_eq!(contents.len(), 1_usize);

        let content = contents.first().expect("contents array non-empty");
        assert!(content.get("uri").expect("uri field exists").is_string());
        assert_eq!(
            content.get("mimeType").expect("mimeType field exists"),
            "application/json"
        );
        assert!(content.get("text").expect("text field exists").is_string());
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
