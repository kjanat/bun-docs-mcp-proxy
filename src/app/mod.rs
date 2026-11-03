//! Application state and MCP server loop.
//!
//! This module contains the main MCP JSON-RPC server loop that handles
//! incoming requests over stdio and dispatches them to the appropriate handlers.

pub mod handlers;

use handlers::{handle_discover, handle_tools_call, handle_tools_list};
use tracing::{Instrument as _, debug, error, info, info_span};

use crate::{
    docs::DocsCorpus,
    io::transport::StdioTransport,
    mcp::{JsonRpcRequest, JsonRpcResponse, Method, error_code, request_error},
    util::truncate_for_log,
};

/// Runs the MCP JSON-RPC server loop over stdio.
///
/// This function reads JSON-RPC requests from stdin and writes responses to stdout.
/// Bun's documentation corpus is loaded when the first search is requested.
/// It handles method dispatch, error responses, and graceful shutdown on EOF.
///
/// # Errors
///
/// Returns an error if transport initialization fails or an unrecoverable
/// I/O error occurs.
#[allow(clippy::too_many_lines, reason = "main event loop is inherently long")]
pub async fn run_mcp_server() -> anyhow::Result<()> {
    info!("Bun Docs MCP server starting");

    let mut transport = StdioTransport::stdio();
    let mut docs = None;

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

        debug!(
            "Raw message ({} bytes): {}",
            message.len(),
            truncate_for_log(&message, 500)
        );

        // Distinguish invalid JSON from a valid JSON value with an invalid RPC shape.
        let parsed: serde_json::Value = match serde_json::from_str(&message) {
            Ok(req) => req,
            Err(e) => {
                error!(
                    "Failed to parse JSON-RPC request: {} | raw: {}",
                    e,
                    truncate_for_log(&message, 200)
                );

                // MCP omits the response ID when no valid request ID is available.
                let error_response = JsonRpcResponse::error(
                    serde_json::Value::Null,
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

        let parsed_id = parsed
            .get("id")
            .filter(|id| id.is_string() || id.is_i64() || id.is_u64())
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let request: JsonRpcRequest = match serde_json::from_value(parsed) {
            Ok(request) => request,
            Err(error) => {
                let response = JsonRpcResponse::error(
                    parsed_id,
                    error_code::INVALID_REQUEST,
                    format!("Invalid Request: {error}"),
                );
                transport
                    .write_message(&serde_json::to_string(&response)?)
                    .await?;
                continue;
            }
        };

        // JSON-RPC 2.0: validate jsonrpc version field
        // Missing or invalid jsonrpc should return -32600 (Invalid Request), not Parse Error
        let jsonrpc_version = request.jsonrpc.as_deref().unwrap_or("");
        if jsonrpc_version != "2.0" {
            let error_response = JsonRpcResponse::error(
                parsed_id,
                error_code::INVALID_REQUEST,
                format!(
                    "Invalid Request: jsonrpc must be exactly \"2.0\", got {:?}",
                    request.jsonrpc
                ),
            );
            if let Ok(response_str) = serde_json::to_string(&error_response)
                && let Err(write_err) = transport.write_message(&response_str).await
            {
                error!("Failed to write invalid request response: {}", write_err);
                break;
            }
            continue;
        }

        // Notifications have no id and never receive a response.
        let Some(request_id) = request.id.clone() else {
            // True notification - no "id" field in request
            debug!("Received notification (no id): {}", request.method);
            continue;
        };

        info!("Received method: {}", request.method);

        // Handle request based on method, wrapped in a span for tracing
        let span = info_span!("mcp_request", id = %request_id, method = %request.method);
        let response = dispatch_request(&mut docs, &request, request_id)
            .instrument(span)
            .await;

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
                break;
            }
        }
    }

    info!("Bun Docs MCP server shutting down");
    Ok(())
}

async fn dispatch_request(
    docs: &mut Option<DocsCorpus>,
    request: &JsonRpcRequest,
    request_id: serde_json::Value,
) -> JsonRpcResponse {
    if let Some(response) = request_error(request, request_id.clone()) {
        return response;
    }
    match request.method.parse::<Method>() {
        Ok(Method::Discover) => handle_discover(request_id),
        Ok(Method::ToolsList) => handle_tools_list(request_id),
        Ok(Method::ToolsCall) => {
            let corpus = if let Some(corpus) = docs {
                corpus
            } else {
                match DocsCorpus::fetch().await {
                    Ok(corpus) => docs.insert(corpus),
                    Err(error) => {
                        return JsonRpcResponse::error(
                            request_id,
                            error_code::INTERNAL_ERROR,
                            format!("Failed to load Bun documentation: {error}"),
                        );
                    }
                }
            };
            handle_tools_call(corpus, request, request_id)
        }
        Err(()) => JsonRpcResponse::error(
            request_id,
            error_code::METHOD_NOT_FOUND,
            format!("Method not found: {}", request.method),
        ),
    }
}
