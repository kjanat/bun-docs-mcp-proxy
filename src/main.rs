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
//! - `tools/list` - List available tools (returns `SearchBun` tool)
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
use std::fmt::Write as FmtWrite;
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
#[command(
    author,
    version,
    about,
    long_about = None,
    after_help = r#"EXAMPLES:
    # Search Bun documentation for "serve" keyword
    bun-docs-mcp-proxy --search "Bun.serve"

    # Save results as markdown
    bun-docs-mcp-proxy -s "HTTP server" -f markdown -o results.md

    # Export as JSON for processing
    bun-docs-mcp-proxy --search "WebSocket" --format json --output ws-docs.json

    # Run as MCP server (default mode, reads from stdin)
    bun-docs-mcp-proxy

ENVIRONMENT:
    RUST_LOG    Set logging level (debug, info, warn, error)
                Example: RUST_LOG=debug bun-docs-mcp-proxy -s "test"

MCP SERVER MODE:
    When run without --search, operates as an MCP (Model Context Protocol) server
    reading JSON-RPC requests from stdin and writing responses to stdout."#
)]
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
        .ok_or_else(|| format!("Missing or invalid {key} parameter"))
}

/// Parse a Bun docs URI and extract the search query
fn parse_bun_docs_uri(uri: &str) -> Result<String, String> {
    if let Some(query_part) = uri.strip_prefix("bun://docs?query=") {
        Ok(query_part.to_string())
    } else if uri == "bun://docs" {
        Ok(String::new())
    } else {
        Err(format!("Invalid URI format: {uri}"))
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

/// Extract text content from a search result
fn extract_content_texts(result: &serde_json::Value) -> Vec<&str> {
    result
        .get("content")
        .and_then(|c| c.as_array())
        .map(|content| {
            content
                .iter()
                .filter_map(|item| item.get("text").and_then(|t| t.as_str()))
                .collect()
        })
        .unwrap_or_default()
}

/// Represents a documentation entry with URL and fallback text
struct DocEntry<'a> {
    url: Option<String>,
    text: &'a str,
}

/// Extract URLs and text from content for markdown fetching
///
/// Parses "Link: URL" patterns from content[].text fields and returns
/// structured entries with both the URL (if found) and the full text as fallback.
fn extract_doc_entries(result: &serde_json::Value) -> Vec<DocEntry<'_>> {
    let texts = extract_content_texts(result);

    texts
        .into_iter()
        .map(|text| {
            // Parse "Link: <URL>" pattern
            let url = text.lines().find_map(|line| {
                let trimmed = line.trim();
                if trimmed.starts_with("Link: ") {
                    Some(trimmed.strip_prefix("Link: ").unwrap().trim().to_string())
                } else {
                    None
                }
            });

            DocEntry { url, text }
        })
        .collect()
}

/// Format search results as JSON
fn format_json(result: &serde_json::Value) -> Result<String> {
    Ok(serde_json::to_string_pretty(result)?)
}

/// Format search results as plain text
fn format_text(result: &serde_json::Value) -> Result<String> {
    let texts = extract_content_texts(result);

    if texts.is_empty() {
        Ok(serde_json::to_string_pretty(result)?)
    } else {
        Ok(texts.join("\n\n"))
    }
}

/// Format search results as markdown with raw MDX fetching
///
/// Extracts URLs from search results, fetches raw MDX source from each URL
/// with `Accept: text/markdown` header, and aggregates the content.
///
/// On fetch failure, falls back to the original text content from search results.
async fn format_markdown(
    result: &serde_json::Value,
    client: &http::BunDocsClient,
) -> Result<String> {
    let doc_entries = extract_doc_entries(result);

    if doc_entries.is_empty() {
        // No content found, fallback to JSON display
        let mut output = String::new();
        output.push_str("```json\n");
        output.push_str(&serde_json::to_string_pretty(result)?);
        output.push_str("\n```\n");
        return Ok(output);
    }

    let mut mdx_parts = Vec::new();

    for entry in doc_entries {
        if let Some(url) = entry.url {
            // Try to fetch MDX from the URL
            match client.fetch_doc_markdown(&url).await {
                Ok(mdx) => {
                    // Success: include URL comment and MDX content
                    let mut part = String::new();
                    write!(part, "<!-- Source: {url} -->\n\n").unwrap();
                    part.push_str(&mdx);
                    mdx_parts.push(part);
                }
                Err(e) => {
                    // Error: include error comment and fallback to original text
                    let mut part = String::new();
                    write!(part, "<!-- Error: {e} -->\n\n").unwrap();
                    part.push_str(entry.text);
                    mdx_parts.push(part);
                    eprintln!("Failed to fetch MDX from {url}: {e}");
                }
            }
        } else {
            // No URL found, use original text content
            mdx_parts.push(entry.text.to_string());
        }
    }

    // Join with horizontal rules and two newlines
    Ok(mdx_parts.join("\n\n---\n\n"))
}

/// Validate output path to prevent directory traversal attacks
fn validate_output_path(path: &str) -> Result<(), String> {
    let path_obj = std::path::Path::new(path);

    // Check for directory traversal attempts
    for component in path_obj.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err("Output path cannot contain '..' (directory traversal)".to_string());
        }
    }

    Ok(())
}

/// Execute a direct search query in CLI mode
async fn direct_search(
    query: &str,
    format: &OutputFormat,
    output_path: Option<&str>,
) -> Result<()> {
    let client = http::BunDocsClient::new();

    // Validate output path if provided
    if let Some(path) = output_path {
        validate_output_path(path).map_err(|e| anyhow::anyhow!("Invalid output path: {e}"))?;

        // Warn if file already exists
        if std::path::Path::new(path).exists() {
            eprintln!("Warning: File '{path}' already exists and will be overwritten");
        }
    }

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

    // Check for API error response
    if let Some(error) = result.get("error") {
        let error_msg = error
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("Unknown error");
        return Err(anyhow::anyhow!("API error: {error_msg}"));
    }

    // Extract result field if present
    let search_result = result.get("result").unwrap_or(&result);

    // Format output
    let formatted = match format {
        OutputFormat::Json => format_json(search_result)?,
        OutputFormat::Text => format_text(search_result)?,
        OutputFormat::Markdown => format_markdown(search_result, &client).await?,
    };

    // Write output
    if let Some(path) = output_path {
        fs::write(path, formatted)?;
        eprintln!("Output written to: {path}");
    } else {
        println!("{formatted}");
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse CLI arguments
    let cli = Cli::parse();

    // Initialize logging early for both CLI and server modes
    init_logging();

    // CLI search mode
    if let Some(query) = &cli.search {
        return direct_search(query, &cli.format, cli.output.as_deref()).await;
    }

    // MCP server mode
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
                    format!("Parse error: {e}"),
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
                    format!("Method not found: {method}"),
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
                format!("Internal error: {e}"),
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
    let Some(params) = &request.params else {
        return JsonRpcResponse::error(
            request.id.clone(),
            JSONRPC_INVALID_PARAMS,
            "Missing params".to_string(),
        );
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
                        format!("Failed to serialize resource: {e}"),
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
                format!("Internal error: {e}"),
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
    fn test_format_json_empty() {
        let result = serde_json::json!({});
        let formatted = format_json(&result).unwrap();
        assert_eq!(formatted, "{}");
    }

    #[test]
    fn test_format_text() {
        let result = serde_json::json!({"content": [{"text": "test content", "type": "text"}]});
        let formatted = format_text(&result).unwrap();
        assert!(formatted.contains("test content"));
        assert!(!formatted.contains("\"content\""));
    }

    #[test]
    fn test_format_text_no_content() {
        let result = serde_json::json!({"other": "data"});
        let formatted = format_text(&result).unwrap();
        assert!(formatted.contains("\"other\""));
        assert!(formatted.contains("\"data\""));
    }

    #[test]
    fn test_format_text_empty_content_array() {
        let result = serde_json::json!({"content": []});
        let formatted = format_text(&result).unwrap();
        // Empty content array falls back to JSON
        assert!(formatted.contains("\"content\": []"));
    }

    #[test]
    fn test_format_text_multiple_items() {
        let result = serde_json::json!({"content": [
            {"text": "first item", "type": "text"},
            {"text": "second item", "type": "text"}
        ]});
        let formatted = format_text(&result).unwrap();
        assert!(formatted.contains("first item"));
        assert!(formatted.contains("second item"));
    }

    #[tokio::test]
    async fn test_format_markdown_no_url() {
        // Test content without URL - should just return the text
        let result = serde_json::json!({"content": [{"text": "test content", "type": "text"}]});
        let client = http::BunDocsClient::new();
        let formatted = format_markdown(&result, &client).await.unwrap();
        assert!(formatted.contains("test content"));
        assert!(!formatted.contains("<!--")); // No URL comment
    }

    #[tokio::test]
    async fn test_format_markdown_no_content() {
        // Test fallback to JSON when no content array
        let result = serde_json::json!({"other": "data"});
        let client = http::BunDocsClient::new();
        let formatted = format_markdown(&result, &client).await.unwrap();
        assert!(formatted.contains("```json"));
        assert!(formatted.contains("\"other\""));
    }

    #[tokio::test]
    async fn test_format_markdown_multiple_items_no_url() {
        // Test multiple items without URLs
        let result = serde_json::json!({"content": [
            {"text": "First Section", "type": "text"},
            {"text": "Second Section", "type": "text"}
        ]});
        let client = http::BunDocsClient::new();
        let formatted = format_markdown(&result, &client).await.unwrap();
        assert!(formatted.contains("First Section"));
        assert!(formatted.contains("Second Section"));
        assert!(formatted.contains("\n\n---\n\n")); // Horizontal rule separator
    }

    #[tokio::test]
    async fn test_format_markdown_empty_content() {
        // Test empty content array falls back to JSON
        let result = serde_json::json!({"content": []});
        let client = http::BunDocsClient::new();
        let formatted = format_markdown(&result, &client).await.unwrap();
        assert!(formatted.contains("```json"));
        assert!(formatted.contains("\"content\": []"));
    }

    #[test]
    fn test_extract_doc_entries_with_url() {
        // Test URL extraction from content
        let result = serde_json::json!({"content": [{
            "text": "Title: Test\nLink: https://example.com/page\nContent: Some content",
            "type": "text"
        }]});
        let entries = extract_doc_entries(&result);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].url.as_ref().unwrap(), "https://example.com/page");
        assert!(entries[0].text.contains("Title: Test"));
    }

    #[test]
    fn test_extract_doc_entries_without_url() {
        // Test content without URL
        let result = serde_json::json!({"content": [{
            "text": "Just some text without a link",
            "type": "text"
        }]});
        let entries = extract_doc_entries(&result);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].url.is_none());
        assert_eq!(entries[0].text, "Just some text without a link");
    }

    #[test]
    fn test_extract_doc_entries_empty() {
        // Test empty content
        let result = serde_json::json!({"content": []});
        let entries = extract_doc_entries(&result);
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn test_extract_doc_entries_multiple_with_mixed_urls() {
        // Test multiple entries, some with URLs, some without
        let result = serde_json::json!({"content": [
            {"text": "Title: First\nLink: https://example.com/first\nContent: text", "type": "text"},
            {"text": "No link here", "type": "text"},
            {"text": "Title: Third\nLink: https://example.com/third", "type": "text"}
        ]});
        let entries = extract_doc_entries(&result);
        assert_eq!(entries.len(), 3);
        assert_eq!(
            entries[0].url.as_ref().unwrap(),
            "https://example.com/first"
        );
        assert!(entries[1].url.is_none());
        assert_eq!(
            entries[2].url.as_ref().unwrap(),
            "https://example.com/third"
        );
    }

    #[test]
    fn test_extract_content_texts_valid() {
        let result = serde_json::json!({"content": [
            {"text": "first", "type": "text"},
            {"text": "second", "type": "text"}
        ]});
        let texts = extract_content_texts(&result);
        assert_eq!(texts, vec!["first", "second"]);
    }

    #[test]
    fn test_extract_content_texts_empty() {
        let result = serde_json::json!({});
        let texts = extract_content_texts(&result);
        assert!(texts.is_empty());
    }

    #[test]
    fn test_extract_content_texts_null_content() {
        let result = serde_json::json!({"content": null});
        let texts = extract_content_texts(&result);
        assert!(texts.is_empty());
    }

    #[test]
    fn test_extract_content_texts_non_array_content() {
        let result = serde_json::json!({"content": "not an array"});
        let texts = extract_content_texts(&result);
        assert!(texts.is_empty());
    }

    #[test]
    fn test_extract_content_texts_missing_text_field() {
        let result = serde_json::json!({"content": [
            {"type": "text"},  // missing text field
            {"text": "valid", "type": "text"}
        ]});
        let texts = extract_content_texts(&result);
        assert_eq!(texts, vec!["valid"]);
    }

    #[test]
    fn test_extract_content_texts_empty_string() {
        let result = serde_json::json!({"content": [
            {"text": "", "type": "text"},
            {"text": "valid", "type": "text"}
        ]});
        let texts = extract_content_texts(&result);
        assert_eq!(texts, vec!["", "valid"]);
    }

    #[test]
    fn test_extract_content_texts_non_string_text() {
        let result = serde_json::json!({"content": [
            {"text": 123, "type": "text"},  // text is number
            {"text": "valid", "type": "text"}
        ]});
        let texts = extract_content_texts(&result);
        assert_eq!(texts, vec!["valid"]);
    }

    #[test]
    fn test_format_text_with_null_content() {
        let result = serde_json::json!({"content": null, "other": "data"});
        let formatted = format_text(&result).unwrap();
        assert!(formatted.contains("\"content\": null"));
    }

    #[tokio::test]
    async fn test_format_markdown_with_null_content() {
        let result = serde_json::json!({"content": null});
        let client = http::BunDocsClient::new();
        let formatted = format_markdown(&result, &client).await.unwrap();
        assert!(formatted.contains("```json"));
        assert!(formatted.contains("null"));
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

    #[tokio::test]
    async fn test_direct_search_json_format() {
        let result = direct_search("Bun.serve", &OutputFormat::Json, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_direct_search_text_format() {
        let result = direct_search("HTTP", &OutputFormat::Text, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_direct_search_markdown_format() {
        let result = direct_search("server", &OutputFormat::Markdown, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_direct_search_with_output_file() {
        let temp_dir = std::env::temp_dir();
        let output_path = temp_dir.join("test_search_output.json");
        let output_str = output_path.to_str().unwrap();

        let result = direct_search("test", &OutputFormat::Json, Some(output_str)).await;
        assert!(result.is_ok());

        // Verify file was created
        assert!(output_path.exists());

        // Read and verify content
        let content = std::fs::read_to_string(&output_path).unwrap();
        assert!(!content.is_empty());

        // Cleanup
        let _ = std::fs::remove_file(&output_path);
    }

    #[tokio::test]
    async fn test_direct_search_empty_query() {
        let result = direct_search("", &OutputFormat::Json, None).await;
        // Should succeed, Bun API handles empty queries
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_direct_search_markdown_with_file() {
        let temp_dir = std::env::temp_dir();
        let output_path = temp_dir.join("test_markdown_output.md");
        let output_str = output_path.to_str().unwrap();

        let result = direct_search("Bun", &OutputFormat::Markdown, Some(output_str)).await;
        assert!(result.is_ok());

        // Verify file was created
        assert!(output_path.exists());

        // Read and verify markdown content (may include URL comments or MDX)
        let content = std::fs::read_to_string(&output_path).unwrap();
        assert!(!content.is_empty(), "Markdown output should not be empty");
        // The content could be raw MDX with URL comments or fallback text

        // Cleanup
        let _ = std::fs::remove_file(&output_path);
    }

    #[test]
    fn test_validate_output_path_valid() {
        assert!(validate_output_path("/tmp/output.json").is_ok());
        assert!(validate_output_path("output.json").is_ok());
        assert!(validate_output_path("./output.json").is_ok());
        assert!(validate_output_path("subdir/output.json").is_ok());
    }

    #[test]
    fn test_validate_output_path_directory_traversal() {
        assert!(validate_output_path("../output.json").is_err());
        assert!(validate_output_path("subdir/../output.json").is_err());
        assert!(validate_output_path("../../etc/passwd").is_err());
    }

    #[tokio::test]
    async fn test_direct_search_invalid_output_path() {
        let result = direct_search("test", &OutputFormat::Json, Some("../output.json")).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("directory traversal")
        );
    }

    #[tokio::test]
    async fn test_direct_search_file_overwrite() {
        let temp_dir = std::env::temp_dir();
        let output_path = temp_dir.join("test_overwrite.json");
        let output_str = output_path.to_str().unwrap();

        // Create existing file
        fs::write(&output_path, "existing content").unwrap();
        assert!(output_path.exists());

        // Should overwrite with warning
        let result = direct_search("test", &OutputFormat::Json, Some(output_str)).await;
        assert!(result.is_ok());

        // Verify new content
        let content = std::fs::read_to_string(&output_path).unwrap();
        assert!(!content.contains("existing content"));

        // Cleanup
        let _ = std::fs::remove_file(&output_path);
    }
}
