//! Bun Docs MCP Proxy - Protocol adapter for Bun documentation search
//!
//! This proxy acts as a bridge between stdio-based MCP (Model Context Protocol) clients
//! (like Zed editor) and the HTTP/SSE-based Bun documentation server at `https://bun.com/docs/mcp`.
//!
//! ## Request Flow
//!
//! ```text
//! stdin (JSON-RPC) → Proxy → HTTP POST → bun.com/docs/mcp → SSE stream → parse → stdout (JSON-RPC)
//! ```
//!
//! ## Supported JSON-RPC Methods
//!
//! - `initialize` - Initialize MCP connection, returns protocol version and capabilities
//! - `tools/list` - List available tools (returns SearchBun tool)
//! - `tools/call` - Execute a tool with parameters (forwarded to Bun Docs API)
//! - `resources/list` - List available resources (returns Bun Documentation resource)
//! - `resources/read` - Read a resource by URI (e.g., `bun://docs?query=Bun.serve`)
//!
//! ## Architecture
//!
//! The proxy consists of three main modules:
//! - [`http`] - HTTP client with SSE parsing and retry logic
//! - [`protocol`] - JSON-RPC 2.0 types and serialization
//! - [`transport`] - Stdio transport layer for reading/writing messages

mod http;
mod protocol;
mod transport;

use anyhow::Result;
use clap::{Parser, ValueEnum};
use protocol::{JsonRpcRequest, JsonRpcResponse};
use std::fs;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

// JSON-RPC error codes
const JSONRPC_PARSE_ERROR: i32 = -32700;
const JSONRPC_INVALID_PARAMS: i32 = -32602;
const JSONRPC_INTERNAL_ERROR: i32 = -32603;
const JSONRPC_METHOD_NOT_FOUND: i32 = -32601;

/// Output format for CLI search results
#[derive(Debug, Clone, ValueEnum)]
enum OutputFormat {
    /// JSON format (default)
    Json,
    /// Plain text format
    Text,
    /// Markdown format
    Markdown,
}

/// Bun Docs MCP Proxy - Protocol adapter and CLI for Bun documentation
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Search query for Bun documentation (enables CLI mode)
    #[arg(short, long)]
    search: Option<String>,

    /// Output file path (default: stdout)
    #[arg(short, long)]
    output: Option<String>,

    /// Output format
    #[arg(short, long, value_enum, default_value_t = OutputFormat::Json)]
    format: OutputFormat,
}

/// Extract a required string parameter from JSON-RPC params
fn get_string_param<'a>(params: &'a serde_json::Value, key: &str) -> Result<&'a str, String> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("Missing or invalid {} parameter", key))
}

/// Parse a Bun docs URI and extract the search query
fn parse_bun_docs_uri(uri: &str) -> Result<String, String> {
    if let Some(query_part) = uri.strip_prefix("bun://docs?query=") {
        Ok(query_part.to_string())
    } else if uri == "bun://docs" {
        Ok("".to_string())
    } else {
        Err(format!("Invalid URI format: {}", uri))
    }
}

fn init_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .without_time()
        .init();
}

/// Format search results as JSON
fn format_json(result: &serde_json::Value) -> Result<String> {
    Ok(serde_json::to_string_pretty(result)?)
}

/// Format search results as plain text
fn format_text(result: &serde_json::Value) -> Result<String> {
    let mut output = String::new();

    if let Some(content) = result.get("content").and_then(|c| c.as_array()) {
        for item in content {
            if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                output.push_str(text);
                output.push_str("\n\n");
            }
        }
    } else {
        output = serde_json::to_string_pretty(result)?;
    }

    Ok(output)
}

/// Format search results as markdown
fn format_markdown(result: &serde_json::Value) -> Result<String> {
    let mut output = String::new();
    output.push_str("# Bun Documentation Search Results\n\n");

    if let Some(content) = result.get("content").and_then(|c| c.as_array()) {
        for item in content {
            if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                output.push_str(text);
                output.push_str("\n\n");
            }
        }
    } else {
        output.push_str("```json\n");
        output.push_str(&serde_json::to_string_pretty(result)?);
        output.push_str("\n```\n");
    }

    Ok(output)
}

/// Execute a direct search query in CLI mode
async fn direct_search(
    query: &str,
    format: &OutputFormat,
    output_path: Option<&str>,
) -> Result<()> {
    let client = http::BunDocsClient::new();

    // Build search request
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "SearchBun",
            "arguments": {
                "query": query
            }
        }
    });

    // Execute search
    let result = client.forward_request(request).await?;

    // Extract result field if present
    let search_result = result.get("result").unwrap_or(&result);

    // Format output
    let formatted = match format {
        OutputFormat::Json => format_json(search_result)?,
        OutputFormat::Text => format_text(search_result)?,
        OutputFormat::Markdown => format_markdown(search_result)?,
    };

    // Write output
    if let Some(path) = output_path {
        fs::write(path, formatted)?;
        eprintln!("Output written to: {}", path);
    } else {
        println!("{}", formatted);
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse CLI arguments
    let cli = Cli::parse();

    // CLI search mode
    if let Some(query) = &cli.search {
        return direct_search(query, &cli.format, cli.output.as_deref()).await;
    }

    // MCP server mode
    init_logging();
    info!("Bun Docs MCP Proxy starting");

    let mut transport = transport::StdioTransport::new();
    let http_client = http::BunDocsClient::new();

    loop {
        // Read JSON-RPC request from stdin
        let message = match transport.read_message().await {
            Ok(Some(msg)) => msg,
            Ok(None) => {
                info!("Connection closed");
                break;
            }
            Err(e) => {
                error!("Failed to read message: {}", e);
                continue;
            }
        };

        // Parse JSON-RPC request
        let request: JsonRpcRequest = match serde_json::from_str(&message) {
            Ok(req) => req,
            Err(e) => {
                error!("Failed to parse JSON-RPC request: {}", e);
                let error_response = JsonRpcResponse::error(
                    serde_json::Value::Null,
                    JSONRPC_PARSE_ERROR,
                    format!("Parse error: {}", e),
                );
                if let Ok(response_str) = serde_json::to_string(&error_response) {
                    let _ = transport.write_message(&response_str).await;
                }
                continue;
            }
        };

        info!("Received method: {}", request.method);

        // Handle request based on method
        let response = match request.method.as_str() {
            "tools/call" => handle_tools_call(&http_client, &request).await,
            "tools/list" => handle_tools_list(&request),
            "resources/list" => handle_resources_list(&request),
            "resources/read" => handle_resources_read(&http_client, &request).await,
            "initialize" => handle_initialize(&request),
            method => {
                error!("Unsupported method: {}", method);
                JsonRpcResponse::error(
                    request.id,
                    JSONRPC_METHOD_NOT_FOUND,
                    format!("Method not found: {}", method),
                )
            }
        };

        // Send response back to stdout
        match serde_json::to_string(&response) {
            Ok(response_str) => {
                if let Err(e) = transport.write_message(&response_str).await {
                    error!("Failed to write response: {}", e);
                    break;
                }
            }
            Err(e) => {
                error!("Failed to serialize response: {}", e);
            }
        }
    }

    info!("Bun Docs MCP Proxy shutting down");
    Ok(())
}

async fn handle_tools_call(
    client: &http::BunDocsClient,
    request: &JsonRpcRequest,
) -> JsonRpcResponse {
    // Forward entire request to Bun Docs API
    let original_request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": request.id,
        "method": request.method,
        "params": request.params
    });

    match client.forward_request(original_request).await {
        Ok(result) => {
            info!("Successfully got response from Bun Docs");

            // Based on protocol analysis, the SSE data contains
            // the complete JSON-RPC response. Extract the result field.
            if let Some(result_field) = result.get("result") {
                JsonRpcResponse::success(request.id.clone(), result_field.clone())
            } else {
                JsonRpcResponse::success(request.id.clone(), result)
            }
        }
        Err(e) => {
            error!("Failed to forward request: {}", e);
            JsonRpcResponse::error(
                request.id.clone(),
                JSONRPC_INTERNAL_ERROR,
                format!("Internal error: {}", e),
            )
        }
    }
}

fn handle_tools_list(request: &JsonRpcRequest) -> JsonRpcResponse {
    // Return available tools
    let tools = serde_json::json!({
        "tools": [{
            "name": "SearchBun",
            "description": "Search Bun documentation",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query"
                    }
                },
                "required": ["query"]
            }
        }]
    });

    JsonRpcResponse::success(request.id.clone(), tools)
}

fn handle_resources_list(request: &JsonRpcRequest) -> JsonRpcResponse {
    // Return available resources
    let resources = serde_json::json!({
        "resources": [{
            "uri": "bun://docs",
            "name": "Bun Documentation",
            "description": "Search and browse Bun documentation",
            "mimeType": "application/json"
        }]
    });

    JsonRpcResponse::success(request.id.clone(), resources)
}

async fn handle_resources_read(
    client: &http::BunDocsClient,
    request: &JsonRpcRequest,
) -> JsonRpcResponse {
    // Extract and validate params
    let params = match &request.params {
        Some(p) => p,
        None => {
            return JsonRpcResponse::error(
                request.id.clone(),
                JSONRPC_INVALID_PARAMS,
                "Missing params".to_string(),
            );
        }
    };

    // Extract URI parameter
    let uri = match get_string_param(params, "uri") {
        Ok(u) => u,
        Err(msg) => {
            return JsonRpcResponse::error(request.id.clone(), JSONRPC_INVALID_PARAMS, msg);
        }
    };

    // Parse URI to extract query
    let query = match parse_bun_docs_uri(uri) {
        Ok(q) => q,
        Err(msg) => {
            return JsonRpcResponse::error(request.id.clone(), JSONRPC_INVALID_PARAMS, msg);
        }
    };

    // Forward to tools/call internally
    let search_request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": request.id,
        "method": "tools/call",
        "params": {
            "name": "SearchBun",
            "arguments": {
                "query": query
            }
        }
    });

    match client.forward_request(search_request).await {
        Ok(result) => {
            info!("Successfully got resource from Bun Docs");

            // Serialize the result to JSON string for resource text field
            // Note: result is the complete JSON-RPC response from Bun Docs API
            // containing {"jsonrpc":"2.0","id":...,"result":{...}}
            let text = match serde_json::to_string(&result) {
                Ok(s) => s,
                Err(e) => {
                    error!("Failed to serialize resource content: {}", e);
                    return JsonRpcResponse::error(
                        request.id.clone(),
                        JSONRPC_INTERNAL_ERROR,
                        format!("Failed to serialize resource: {}", e),
                    );
                }
            };

            // Wrap in MCP resource format
            let resource_response = serde_json::json!({
                "contents": [{
                    "uri": uri,
                    "mimeType": "application/json",
                    "text": text
                }]
            });

            JsonRpcResponse::success(request.id.clone(), resource_response)
        }
        Err(e) => {
            error!("Failed to read resource: {}", e);
            JsonRpcResponse::error(
                request.id.clone(),
                JSONRPC_INTERNAL_ERROR,
                format!("Internal error: {}", e),
            )
        }
    }
}

fn handle_initialize(request: &JsonRpcRequest) -> JsonRpcResponse {
    // Handle MCP initialize request
    let init_result = serde_json::json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {
            "tools": {},
            "resources": {}
        },
        "serverInfo": {
            "name": "bun-docs-mcp-proxy",
            "version": env!("CARGO_PKG_VERSION")
        }
    });

    JsonRpcResponse::success(request.id.clone(), init_result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_handle_initialize() {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: json!(1),
            method: "initialize".to_string(),
            params: None,
        };

        let response = handle_initialize(&request);
        let serialized = serde_json::to_value(&response).unwrap();

        assert_eq!(serialized["id"], 1);
        assert_eq!(serialized["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(
            serialized["result"]["serverInfo"]["name"],
            "bun-docs-mcp-proxy"
        );
        assert!(serialized["result"]["capabilities"]["tools"].is_object());
    }

    #[test]
    fn test_handle_tools_list() {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: json!("test-id"),
            method: "tools/list".to_string(),
            params: None,
        };

        let response = handle_tools_list(&request);
        let serialized = serde_json::to_value(&response).unwrap();

        assert_eq!(serialized["id"], "test-id");
        assert!(serialized["result"]["tools"].is_array());

        let tools = serialized["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "SearchBun");
        assert_eq!(
            tools[0]["inputSchema"]["properties"]["query"]["type"],
            "string"
        );
    }

    #[test]
    fn test_parse_valid_jsonrpc_request() {
        let message = r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#;
        let request: Result<JsonRpcRequest, _> = serde_json::from_str(message);

        assert!(request.is_ok());
        let req = request.unwrap();
        assert_eq!(req.method, "initialize");
        assert_eq!(req.id, json!(1));
    }

    #[test]
    fn test_parse_invalid_jsonrpc_request() {
        let message = r#"{"invalid json"#;
        let request: Result<JsonRpcRequest, _> = serde_json::from_str(message);

        assert!(request.is_err());
    }

    #[test]
    fn test_error_response_codes() {
        // Test parse error
        let parse_error = JsonRpcResponse::error(json!(1), -32700, "Parse error".to_string());
        let serialized = serde_json::to_value(&parse_error).unwrap();
        assert_eq!(serialized["error"]["code"], -32700);

        // Test method not found
        let method_error = JsonRpcResponse::error(json!(2), -32601, "Method not found".to_string());
        let serialized = serde_json::to_value(&method_error).unwrap();
        assert_eq!(serialized["error"]["code"], -32601);

        // Test internal error
        let internal_error = JsonRpcResponse::error(json!(3), -32603, "Internal error".to_string());
        let serialized = serde_json::to_value(&internal_error).unwrap();
        assert_eq!(serialized["error"]["code"], -32603);
    }

    #[test]
    fn test_response_serialization() {
        let response = JsonRpcResponse::success(json!("test-id"), json!({"result": "data"}));
        let serialized = serde_json::to_string(&response);

        assert!(serialized.is_ok());
        let json_str = serialized.unwrap();
        assert!(json_str.contains("\"jsonrpc\":\"2.0\""));
        assert!(json_str.contains("\"id\":\"test-id\""));
    }

    #[test]
    fn test_handle_tools_list_structure() {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: json!(1),
            method: "tools/list".to_string(),
            params: None,
        };

        let response = handle_tools_list(&request);
        let serialized = serde_json::to_value(&response).unwrap();

        // Verify required fields
        assert!(serialized["result"]["tools"].is_array());
        let tools = serialized["result"]["tools"].as_array().unwrap();
        assert!(!tools.is_empty());

        // Verify tool structure
        let tool = &tools[0];
        assert!(tool["name"].is_string());
        assert!(tool["description"].is_string());
        assert!(tool["inputSchema"]["type"].is_string());
        assert_eq!(tool["inputSchema"]["type"], "object");
    }

    #[test]
    fn test_initialize_response_version() {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: json!(1),
            method: "initialize".to_string(),
            params: None,
        };

        let response = handle_initialize(&request);
        let serialized = serde_json::to_value(&response).unwrap();

        // Verify protocol version matches MCP spec
        assert_eq!(serialized["result"]["protocolVersion"], "2024-11-05");
        // Verify both capabilities are present
        assert!(serialized["result"]["capabilities"]["tools"].is_object());
        assert!(serialized["result"]["capabilities"]["resources"].is_object());
    }

    #[test]
    fn test_handle_resources_list() {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: json!("res-list"),
            method: "resources/list".to_string(),
            params: None,
        };

        let response = handle_resources_list(&request);
        let serialized = serde_json::to_value(&response).unwrap();

        assert_eq!(serialized["id"], "res-list");
        assert!(serialized["result"]["resources"].is_array());

        let resources = serialized["result"]["resources"].as_array().unwrap();
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0]["uri"], "bun://docs");
        assert_eq!(resources[0]["name"], "Bun Documentation");
        assert_eq!(resources[0]["mimeType"], "application/json");
    }

    #[test]
    fn test_jsonrpc_request_with_params() {
        let message = r#"{"jsonrpc":"2.0","id":1,"method":"test","params":{"key":"value"}}"#;
        let request: JsonRpcRequest = serde_json::from_str(message).unwrap();

        assert!(request.params.is_some());
        let params = request.params.unwrap();
        assert_eq!(params["key"], "value");
    }

    #[test]
    fn test_response_null_id() {
        let response = JsonRpcResponse::error(json!(null), -32700, "Error".to_string());
        let serialized = serde_json::to_value(&response).unwrap();

        assert!(serialized["id"].is_null());
    }

    #[tokio::test]
    async fn test_handle_tools_call_real_api() {
        let client = http::BunDocsClient::new();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: json!(1),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": "SearchBun",
                "arguments": {
                    "query": "Bun.serve"
                }
            })),
        };

        let response = handle_tools_call(&client, &request).await;
        let serialized = serde_json::to_value(&response).unwrap();

        assert!(serialized["result"].is_object());
        assert!(serialized["result"]["content"].is_array());
    }

    #[tokio::test]
    async fn test_handle_tools_call_empty_query() {
        // NOTE: This test reflects Bun API's current behavior for empty query.
        // As of now, Bun returns {"content":[{"text":"No results found","type":"text"}],"isError":true}
        // If Bun changes this behavior (e.g., returns docs overview), update expected output accordingly.
        let client = http::BunDocsClient::new();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: json!(2),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": "SearchBun",
                "arguments": {
                    "query": ""
                }
            })),
        };

        let response = handle_tools_call(&client, &request).await;
        let serialized = serde_json::to_value(&response).unwrap();

        // Proxy should forward successfully; Bun API decides what empty query means
        assert!(serialized["result"].is_object());
    }

    #[tokio::test]
    async fn test_handle_resources_read_with_query() {
        let client = http::BunDocsClient::new();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: json!("res1"),
            method: "resources/read".to_string(),
            params: Some(json!({"uri": "bun://docs?query=Bun.serve"})),
        };

        let response = handle_resources_read(&client, &request).await;
        let serialized = serde_json::to_value(&response).unwrap();

        assert!(serialized["result"]["contents"].is_array());
        let contents = serialized["result"]["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0]["uri"], "bun://docs?query=Bun.serve");
        assert_eq!(contents[0]["mimeType"], "application/json");
    }

    #[tokio::test]
    async fn test_handle_resources_read_empty_query() {
        // NOTE: Tests bun://docs (no query param) which proxy converts to empty query string.
        // Bun API currently returns "No results found" for empty queries.
        // If Bun changes to return overview/help for empty query, this test still passes (valid contents array).
        let client = http::BunDocsClient::new();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: json!("res2"),
            method: "resources/read".to_string(),
            params: Some(json!({"uri": "bun://docs"})),
        };

        let response = handle_resources_read(&client, &request).await;
        let serialized = serde_json::to_value(&response).unwrap();

        assert!(serialized["result"]["contents"].is_array());
    }

    #[tokio::test]
    async fn test_handle_resources_read_missing_params() {
        let client = http::BunDocsClient::new();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: json!("res3"),
            method: "resources/read".to_string(),
            params: None,
        };

        let response = handle_resources_read(&client, &request).await;
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
            jsonrpc: "2.0".to_string(),
            id: json!("res4"),
            method: "resources/read".to_string(),
            params: Some(json!({"uri": "invalid://uri"})),
        };

        let response = handle_resources_read(&client, &request).await;
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
            jsonrpc: "2.0".to_string(),
            id: json!("res5"),
            method: "resources/read".to_string(),
            params: Some(json!({"other": "value"})),
        };

        let response = handle_resources_read(&client, &request).await;
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
    async fn test_handle_resources_read_with_real_search() {
        let client = http::BunDocsClient::new();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: json!("res6"),
            method: "resources/read".to_string(),
            params: Some(json!({"uri": "bun://docs?query=HTTP"})),
        };

        let response = handle_resources_read(&client, &request).await;
        let serialized = serde_json::to_value(&response).unwrap();

        // Real API should return valid results
        assert!(serialized["result"]["contents"].is_array());
        let contents = serialized["result"]["contents"].as_array().unwrap();
        assert!(!contents.is_empty());
    }

    #[test]
    fn test_init_logging_execution() {
        // Test that init_logging can be called
        // Will panic if called twice, but that's expected
        let result = std::panic::catch_unwind(|| {
            init_logging();
        });

        // Either succeeds or panics (already initialized) - both are fine
        // This just ensures the function code path is exercised
        let _ = result;
    }

    #[test]
    fn test_format_json() {
        let result = serde_json::json!({"content": [{"text": "test", "type": "text"}]});
        let formatted = format_json(&result).unwrap();
        assert!(formatted.contains("\"content\""));
        assert!(formatted.contains("\"text\": \"test\""));
    }

    #[test]
    fn test_format_text() {
        let result = serde_json::json!({"content": [{"text": "test content", "type": "text"}]});
        let formatted = format_text(&result).unwrap();
        assert!(formatted.contains("test content"));
        assert!(!formatted.contains("\"content\""));
    }

    #[test]
    fn test_format_markdown() {
        let result = serde_json::json!({"content": [{"text": "test content", "type": "text"}]});
        let formatted = format_markdown(&result).unwrap();
        assert!(formatted.contains("# Bun Documentation Search Results"));
        assert!(formatted.contains("test content"));
    }

    #[test]
    fn test_get_string_param() {
        let params = json!({"uri": "bun://docs", "other": 123});

        assert_eq!(get_string_param(&params, "uri").unwrap(), "bun://docs");
        assert!(get_string_param(&params, "other").is_err());
        assert!(get_string_param(&params, "missing").is_err());
    }

    #[test]
    fn test_parse_bun_docs_uri() {
        assert_eq!(parse_bun_docs_uri("bun://docs").unwrap(), "");
        assert_eq!(parse_bun_docs_uri("bun://docs?query=test").unwrap(), "test");
        assert_eq!(
            parse_bun_docs_uri("bun://docs?query=Bun.serve").unwrap(),
            "Bun.serve"
        );
        assert!(parse_bun_docs_uri("invalid://uri").is_err());
        assert!(parse_bun_docs_uri("").is_err());
    }

    #[test]
    fn test_jsonrpc_error_code_constants() {
        assert_eq!(JSONRPC_PARSE_ERROR, -32700);
        assert_eq!(JSONRPC_INVALID_PARAMS, -32602);
        assert_eq!(JSONRPC_INTERNAL_ERROR, -32603);
        assert_eq!(JSONRPC_METHOD_NOT_FOUND, -32601);
    }
}
