//! Bun Docs MCP Proxy.
//!
//! Two modes:
//! - **MCP server mode** (default): reads JSON-RPC over stdin/stdout, proxies to bun.com/docs/mcp.
//! - **CLI mode** (`--search ...`): performs a Bun docs query and prints/writes results.
//!
//! ## Request Flow (MCP mode)
//!
//! ```text
//! stdin (JSON-RPC) -> Proxy -> HTTP POST -> bun.com/docs/mcp -> SSE stream -> parse -> stdout (JSON-RPC)
//! ```
//!
//! Upstream transport: HTTP POST returning JSON or SSE; downstream transport: stdio JSON-RPC.

mod constants;
mod http;
mod protocol;
mod transport;
mod util;

use anyhow::Result;
use clap::{Parser, ValueEnum};
use constants::{
    BUN_URI_HOST, BUN_URI_SCHEME, LINK_MARKER, MCP_PROTOCOL_VERSION, Method, SERVER_NAME,
    content_type, error_code,
};
use core::fmt::Write as _;
use protocol::{JsonRpcRequest, JsonRpcResponse};
use std::fs;
use tracing::{debug, error, info, warn};
use tracing_subscriber::EnvFilter;

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

// Re-export truncate_for_log from util module
use util::truncate_for_log;

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
fn extract_id_from_json(json_str: &str) -> serde_json::Value {
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

/// Initializes the `tracing` subscriber for logging.
///
/// This function sets up `tracing_subscriber` to filter logs based on the `RUST_LOG`
/// environment variable (defaulting to `info` if not set) and directs output to `stderr`.
fn init_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .without_time()
        .init();
}

/// Extracts all text content from a search result's `content` array.
///
/// The search result is expected to be a JSON object with a `content` field,
/// which is an array of objects, each with a `text` field.
///
/// # Arguments
/// * `result` - A reference to the `serde_json::Value` representing the search result.
///
/// # Returns
/// A `Vec<&str>` containing all the extracted text slices. Returns an empty vector
/// if `content` is missing or not an array.
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

/// Represents a single documentation entry, which may have a URL for fetching the
/// full content and always has fallback text from the initial search result.
struct DocEntry<'text> {
    /// An optional URL to the full documentation page.
    url: Option<String>,
    /// The fallback text content, extracted from the search result.
    text: &'text str,
}

/// Parses search result content to create a vector of `DocEntry` structs.
///
/// This function iterates through the text content of a search result, looking for
/// `Link:` annotations to extract URLs. It creates a `DocEntry` for each piece of
/// content, containing the URL (if found) and the original text as a fallback.
///
/// # Arguments
/// * `result` - A reference to the `serde_json::Value` representing the search result.
///
/// # Returns
/// A `Vec<DocEntry>` containing the parsed documentation entries.
fn extract_doc_entries(result: &serde_json::Value) -> Vec<DocEntry<'_>> {
    let texts = extract_content_texts(result);

    texts
        .into_iter()
        .map(|text| {
            // Parse "Link: <URL>" pattern
            let url = text.lines().find_map(|line| {
                let trimmed = line.trim();
                trimmed
                    .strip_prefix(LINK_MARKER)
                    .map(|url_part| url_part.trim().to_owned())
            });

            DocEntry { url, text }
        })
        .collect()
}

/// Formats a search result as a pretty-printed JSON string.
///
/// # Arguments
/// * `result` - A reference to the `serde_json::Value` to format.
///
/// # Returns
/// A `Result` containing the formatted JSON string, or an error if serialization fails.
fn format_json(result: &serde_json::Value) -> Result<String> {
    Ok(serde_json::to_string_pretty(result)?)
}

/// Formats a search result as a plain text string.
///
/// It extracts the text content from the result and joins it with newlines.
/// If no text content is found, it falls back to a pretty-printed JSON representation.
///
/// # Arguments
/// * `result` - A reference to the `serde_json::Value` to format.
///
/// # Returns
/// A `Result` containing the formatted plain text string.
fn format_text(result: &serde_json::Value) -> Result<String> {
    let texts = extract_content_texts(result);

    if texts.is_empty() {
        Ok(serde_json::to_string_pretty(result)?)
    } else {
        Ok(texts.join("\n\n"))
    }
}

/// Formats a search result as a Markdown string by fetching the raw MDX content from URLs.
///
/// This function extracts `DocEntry` items from the search result. For each entry with a URL,
/// it attempts to fetch the full MDX content. If successful, the content is included with a source
/// comment. If the fetch fails or no URL is present, it falls back to the entry's text.
/// The final output joins all parts with Markdown horizontal rules.
///
/// # Arguments
/// * `result` - A reference to the `serde_json::Value` representing the search result.
/// * `client` - A reference to the `BunDocsClient` for fetching MDX content.
///
/// # Returns
/// A `Result` containing the aggregated and formatted Markdown string.
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
            let fetch_result = client.fetch_doc_markdown(&url).await;
            match fetch_result {
                Ok(mdx) => {
                    // Success: include URL comment and MDX content
                    let mut part = String::new();
                    write!(part, "<!-- Source: {url} -->\n\n")?;
                    part.push_str(&mdx);
                    mdx_parts.push(part);
                }
                Err(e) => {
                    // Error: include error comment and fallback to original text
                    warn!("Failed to fetch MDX from {url}: {e}");
                    let mut part = String::new();
                    write!(part, "<!-- Error: {e} -->\n\n")?;
                    part.push_str(entry.text);
                    mdx_parts.push(part);
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

/// Validates a file path to ensure it does not contain directory traversal components (e.g., `..`).
///
/// This is a security measure to prevent writing files outside of the intended directory.
///
/// # Arguments
/// * `path` - The file path string to validate.
///
/// # Returns
/// An `Ok(())` if the path is valid, or an `Err(String)` if it contains traversal components.
fn validate_output_path(path: &str) -> Result<(), String> {
    let path_obj = std::path::Path::new(path);

    // Reject absolute paths
    if path_obj.is_absolute() {
        return Err("Output path must be relative".to_owned());
    }

    // Check for directory traversal attempts
    for component in path_obj.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err("Output path cannot contain '..' (directory traversal)".to_owned());
        }
    }

    Ok(())
}

/// Executes a search query in CLI mode, formats the result, and writes it to the specified output.
///
/// This function orchestrates the CLI search functionality. It builds and sends a `tools/call`
/// request to the Bun Docs API, formats the response according to the user's choice
/// (JSON, text, or Markdown), and writes the output to a file or `stdout`.
///
/// # Arguments
/// * `query` - The search query string.
/// * `format` - The desired `OutputFormat` for the results.
/// * `output_path` - An optional file path to write the output to. If `None`, output is written to `stdout`.
///
/// # Returns
/// An `anyhow::Result<()>` indicating success or failure.
async fn direct_search(
    query: &str,
    format: &OutputFormat,
    output_path: Option<&str>,
) -> Result<()> {
    // Validate query is not empty
    if query.trim().is_empty() {
        return Err(anyhow::anyhow!("Search query cannot be empty"));
    }

    let client = http::BunDocsClient::new();

    // Validate output path if provided
    if let Some(path) = output_path {
        validate_output_path(path).map_err(|e| anyhow::anyhow!("Invalid output path: {e}"))?;
    }

    // Build search request
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1_i32,
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
        let bytes_written = formatted.len();
        fs::write(path, &formatted)?;
        eprintln!("Output written to: {path} ({bytes_written} bytes)");
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

    let mut transport = transport::StdioTransport::stdio();
    let http_client = http::BunDocsClient::new();

    loop {
        // Read JSON-RPC request from stdin
        let read_result = transport.read_message().await;
        let message = match read_result {
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

        // Log raw message for debugging parse errors
        debug!(
            "Raw message ({} bytes): {}",
            message.len(),
            truncate_for_log(&message, 500)
        );

        // Parse JSON-RPC request
        let request: JsonRpcRequest = match serde_json::from_str(&message) {
            Ok(req) => req,
            Err(e) => {
                error!(
                    "Failed to parse JSON-RPC request: {} | raw: {}",
                    e,
                    truncate_for_log(&message, 200)
                );

                // Try to extract id from malformed JSON for better error correlation
                let extracted_id = extract_id_from_json(&message);

                let error_response = JsonRpcResponse::error(
                    extracted_id,
                    error_code::PARSE_ERROR,
                    format!("Parse error: {e}"),
                );
                if let Ok(response_str) = serde_json::to_string(&error_response)
                    && let Err(write_err) = transport.write_message(&response_str).await
                {
                    error!("Failed to write parse error response: {}", write_err);
                    break;
                }
                continue;
            }
        };

        // JSON-RPC 2.0: notifications have no id field - don't send a response
        let Some(request_id) = request.id.clone() else {
            debug!("Received notification (no id): {}", request.method);
            continue;
        };

        info!("Received method: {}", request.method);

        // Handle request based on method
        let response = match request.method.parse::<Method>() {
            Ok(Method::ToolsCall) => {
                handle_tools_call(&http_client, &request, request_id.clone()).await
            }
            Ok(Method::ToolsList) => handle_tools_list(request_id.clone()),
            Ok(Method::ResourcesList) => handle_resources_list(request_id.clone()),
            Ok(Method::ResourcesRead) => {
                handle_resources_read(&http_client, &request, request_id.clone()).await
            }
            Ok(Method::Initialize) => handle_initialize(request_id.clone()),
            Ok(Method::NotificationsInitialized) => {
                // This shouldn't happen since notifications have no id, but handle gracefully
                debug!("Unexpected notifications/initialized with id");
                continue;
            }
            Err(()) => {
                error!("Unsupported method: {}", request.method);
                JsonRpcResponse::error(
                    request_id,
                    error_code::METHOD_NOT_FOUND,
                    format!("Method not found: {}", request.method),
                )
            }
        };

        // Send response back to stdout
        let serialize_result = serde_json::to_string(&response);
        match serialize_result {
            Ok(response_str) => {
                let write_result = transport.write_message(&response_str).await;
                if let Err(e) = write_result {
                    error!("Failed to write response: {}", e);
                    break;
                }
            }
            Err(e) => {
                error!("Failed to serialize response: {}", e);
                // Serialization failures are likely unrecoverable (e.g., internal data corruption)
                break;
            }
        }
    }

    info!("Bun Docs MCP Proxy shutting down");
    Ok(())
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
async fn handle_tools_call(
    client: &http::BunDocsClient,
    request: &JsonRpcRequest,
    request_id: serde_json::Value,
) -> JsonRpcResponse {
    // Forward entire request to Bun Docs API
    let original_request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": request.method,
        "params": request.params
    });

    match client.forward_request(original_request).await {
        Ok(result) => {
            info!("Successfully got response from Bun Docs");

            // Based on protocol analysis, the SSE data contains
            // the complete JSON-RPC response. Extract the result field.
            #[allow(
                clippy::option_if_let_else,
                reason = "clearer with explicit pattern match"
            )]
            if let Some(result_field) = result.get("result") {
                JsonRpcResponse::success(request_id, result_field.clone())
            } else {
                JsonRpcResponse::success(request_id, result)
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

/// Handles a `tools/list` JSON-RPC request by returning a static list of available tools.
///
/// Currently, this returns a single tool: `SearchBun`.
///
/// # Arguments
/// * `request_id` - The request identifier for the response.
///
/// # Returns
/// A `JsonRpcResponse` containing the list of tools.
fn handle_tools_list(request_id: serde_json::Value) -> JsonRpcResponse {
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

    JsonRpcResponse::success(request_id, tools)
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
fn handle_resources_list(request_id: serde_json::Value) -> JsonRpcResponse {
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
async fn handle_resources_read(
    client: &http::BunDocsClient,
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
        Ok(u) => u,
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

/// Handles an `initialize` JSON-RPC request by returning the protocol version,
/// capabilities, and server information.
///
/// # Arguments
/// * `request_id` - The request identifier for the response.
///
/// # Returns
/// A `JsonRpcResponse` containing the initialization result.
fn handle_initialize(request_id: serde_json::Value) -> JsonRpcResponse {
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

#[cfg(test)]
mod main_tests;
