//! Application state and MCP server loop.
//!
//! This module contains the main MCP JSON-RPC server loop that handles
//! incoming requests over stdio and dispatches them to the appropriate handlers.

pub mod handlers;

use handlers::{handle_initialize, handle_tools_call, handle_tools_list};
use tracing::{Instrument as _, debug, error, info, info_span};

use crate::{
    io::transport::StdioTransport,
    mcp::{JsonRpcRequest, JsonRpcResponse, Method, error_code},
    upstream::BunDocsClient,
    util::truncate_for_log,
};

/// Runs the MCP JSON-RPC server loop over stdio.
///
/// This function initializes the transport and HTTP client, then enters an infinite
/// loop reading JSON-RPC requests from stdin and writing responses to stdout.
/// It handles method dispatch, error responses, and graceful shutdown on EOF.
///
/// # Errors
///
/// Returns an error if transport initialization fails or an unrecoverable
/// I/O error occurs.
#[allow(clippy::too_many_lines, reason = "main event loop is inherently long")]
pub async fn run_mcp_server() -> anyhow::Result<()> {
    info!("Bun Docs MCP Proxy starting");

    let mut transport = StdioTransport::stdio();
    let http_client = BunDocsClient::new();

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

        // Parse JSON-RPC request
        let request: JsonRpcRequest = match serde_json::from_str(&message) {
            Ok(req) => req,
            Err(e) => {
                error!(
                    "Failed to parse JSON-RPC request: {} | raw: {}",
                    e,
                    truncate_for_log(&message, 200)
                );

                // JSON-RPC 2.0 spec: parse errors MUST use id: null
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

        // JSON-RPC 2.0: validate jsonrpc version field
        // Missing or invalid jsonrpc should return -32600 (Invalid Request), not Parse Error
        let jsonrpc_version = request.jsonrpc.as_deref().unwrap_or("");
        if jsonrpc_version != "2.0" {
            let request_id = request.id.clone().unwrap_or(serde_json::Value::Null);
            let error_response = JsonRpcResponse::error(
                request_id,
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

        // JSON-RPC 2.0: notifications have no id field - don't send a response
        // Note: id: null is a valid request (requires response), only missing id is a notification
        let Some(request_id) = request.id.clone() else {
            // True notification - no "id" field in request
            debug!("Received notification (no id): {}", request.method);
            continue;
        };

        info!("Received method: {}", request.method);

        // Handle request based on method, wrapped in a span for tracing
        let span = info_span!("mcp_request", id = %request_id, method = %request.method);
        let response = async {
            match request.method.parse::<Method>() {
                Ok(Method::ToolsCall) => {
                    handle_tools_call(&http_client, &request, request_id.clone()).await
                }
                Ok(Method::ToolsList) => handle_tools_list(&http_client, request_id.clone()).await,
                Ok(Method::Initialize) => {
                    handle_initialize(&http_client, &request, request_id.clone()).await
                }
                Ok(Method::NotificationsInitialized) => {
                    debug!("Unexpected notifications/initialized with id");
                    JsonRpcResponse::success(request_id.clone(), serde_json::Value::Null)
                }
                Err(()) => {
                    error!("Unsupported method: {}", request.method);
                    JsonRpcResponse::error(
                        request_id,
                        error_code::METHOD_NOT_FOUND,
                        format!("Method not found: {}", request.method),
                    )
                }
            }
        }
        .instrument(span)
        .await;

        if request.method == "notifications/initialized" {
            continue;
        }

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

    info!("Bun Docs MCP Proxy shutting down");
    Ok(())
}
