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
use core::fmt::Write as _;
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
        return Ok(query_part.to_owned());
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
                    return Some(trimmed.strip_prefix("Link: ").unwrap().trim().to_owned());
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
            mdx_parts.push(entry.text.to_owned());
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
            return Err("Output path cannot contain '..' (directory traversal)".to_owned());
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
            "Missing params".to_owned(),
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
mod main_tests;
