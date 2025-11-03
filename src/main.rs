mod http;
mod protocol;
mod transport;

use anyhow::Result;
use protocol::{JsonRpcRequest, JsonRpcResponse};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

fn init_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .without_time()
        .init();
}

#[tokio::main]
async fn main() -> Result<()> {
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
                    -32700,
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
            "initialize" => handle_initialize(&request),
            method => {
                error!("Unsupported method: {}", method);
                JsonRpcResponse::error(request.id, -32601, format!("Method not found: {}", method))
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
            JsonRpcResponse::error(request.id.clone(), -32603, format!("Internal error: {}", e))
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

fn handle_initialize(request: &JsonRpcRequest) -> JsonRpcResponse {
    // Handle MCP initialize request
    let init_result = serde_json::json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {
            "tools": {}
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
        let request: Result<JsonRpcRequest, _> = serde_json::from_str(&message);

        assert!(request.is_ok());
        let req = request.unwrap();
        assert_eq!(req.method, "initialize");
        assert_eq!(req.id, json!(1));
    }

    #[test]
    fn test_parse_invalid_jsonrpc_request() {
        let message = r#"{"invalid json"#;
        let request: Result<JsonRpcRequest, _> = serde_json::from_str(&message);

        assert!(request.is_err());
    }
}
