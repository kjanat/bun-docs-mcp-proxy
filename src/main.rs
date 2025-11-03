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

fn print_version() {
    println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
}

fn print_help() {
    println!(
        "{} {} - {}

USAGE:
    {} [FLAGS]

FLAGS:
    -h, --help       Print help information
    -V, --version    Print version information

DESCRIPTION:
    MCP (Model Context Protocol) proxy for Bun documentation search.

    Acts as a protocol adapter that receives JSON-RPC 2.0 requests over stdin,
    forwards them to the Bun documentation HTTP API at https://bun.com/docs/mcp,
    parses SSE (Server-Sent Events) responses, and returns JSON-RPC responses
    over stdout.

SUPPORTED METHODS:
    initialize       Initialize the MCP connection
    tools/list       List available tools (SearchBun)
    tools/call       Call a tool with parameters
    resources/list   List available resources (Bun Documentation)
    resources/read   Read a resource by URI

ENVIRONMENT VARIABLES:
    RUST_LOG         Set logging level (debug, info, warn, error)

EXAMPLES:
    # Start the proxy (typically called by MCP client)
    {}

    # Start with debug logging
    RUST_LOG=debug {}

    # Test tools/call method
    echo '{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{{\"name\":\"SearchBun\",\"arguments\":{{\"query\":\"Bun.serve\"}}}}}}' | {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_DESCRIPTION"),
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_NAME")
    );
}

fn handle_args(args: &[String]) -> bool {
    // Check for flags (skip first arg which is program name)
    if let Some(arg) = args.get(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                return true;
            }
            "-V" | "--version" => {
                print_version();
                return true;
            }
            _ => {
                eprintln!("Unknown argument: {}", arg);
                eprintln!("Use --help for usage information");
                std::process::exit(1);
            }
        }
    }

    false
}

#[tokio::main]
async fn main() -> Result<()> {
    // Handle command-line arguments before starting proxy
    let args: Vec<String> = std::env::args().collect();
    if handle_args(&args) {
        return Ok(());
    }

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
            "resources/list" => handle_resources_list(&request),
            "resources/read" => handle_resources_read(&http_client, &request).await,
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
    // Extract query from params
    let params = match &request.params {
        Some(p) => p,
        None => {
            return JsonRpcResponse::error(
                request.id.clone(),
                -32602,
                "Missing params".to_string(),
            );
        }
    };

    let uri = match params.get("uri") {
        Some(u) if u.is_string() => u.as_str().unwrap(),
        _ => {
            return JsonRpcResponse::error(
                request.id.clone(),
                -32602,
                "Missing or invalid uri parameter".to_string(),
            );
        }
    };

    // Extract query from URI (e.g., bun://docs?query=Bun.serve)
    let query = if let Some(query_part) = uri.strip_prefix("bun://docs?query=") {
        query_part.to_string()
    } else if uri == "bun://docs" {
        // Default query
        "".to_string()
    } else {
        return JsonRpcResponse::error(
            request.id.clone(),
            -32602,
            format!("Invalid URI format: {}", uri),
        );
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

            // Wrap in resource format
            let resource_response = serde_json::json!({
                "contents": [{
                    "uri": uri,
                    "mimeType": "application/json",
                    "text": serde_json::to_string(&result).unwrap_or_default()
                }]
            });

            JsonRpcResponse::success(request.id.clone(), resource_response)
        }
        Err(e) => {
            error!("Failed to read resource: {}", e);
            JsonRpcResponse::error(request.id.clone(), -32603, format!("Internal error: {}", e))
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
    async fn test_handle_tools_call_with_result_extraction() {
        let mut server = mockito::Server::new_async().await;

        let _mock = server
            .mock("POST", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"result": {"data": "extracted"}}"#)
            .create_async()
            .await;

        let client = http::BunDocsClient::new_with_url(server.url());
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: json!(1),
            method: "tools/call".to_string(),
            params: Some(json!({"name": "SearchBun"})),
        };

        let response = handle_tools_call(&client, &request).await;
        let serialized = serde_json::to_value(&response).unwrap();

        assert!(serialized["result"].is_object());
    }

    #[tokio::test]
    async fn test_handle_tools_call_without_result_field() {
        let mut server = mockito::Server::new_async().await;

        let _mock = server
            .mock("POST", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"data": "no result field"}"#)
            .create_async()
            .await;

        let client = http::BunDocsClient::new_with_url(server.url());
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: json!(2),
            method: "tools/call".to_string(),
            params: None,
        };

        let response = handle_tools_call(&client, &request).await;
        let serialized = serde_json::to_value(&response).unwrap();

        assert!(serialized["result"]["data"].is_string());
    }

    #[tokio::test]
    async fn test_handle_tools_call_http_error() {
        let mut server = mockito::Server::new_async().await;

        let _mock = server
            .mock("POST", "/")
            .with_status(503)
            .with_body("Service Unavailable")
            .create_async()
            .await;

        let client = http::BunDocsClient::new_with_url(server.url());
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: json!(3),
            method: "tools/call".to_string(),
            params: None,
        };

        let response = handle_tools_call(&client, &request).await;
        let serialized = serde_json::to_value(&response).unwrap();

        assert!(serialized["error"].is_object());
        assert_eq!(serialized["error"]["code"], -32603);
    }

    #[tokio::test]
    async fn test_handle_resources_read_with_query() {
        let mut server = mockito::Server::new_async().await;

        let _mock = server
            .mock("POST", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"result": {"data": "bun serve docs"}}"#)
            .create_async()
            .await;

        let client = http::BunDocsClient::new_with_url(server.url());
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
        let mut server = mockito::Server::new_async().await;

        let _mock = server
            .mock("POST", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"result": {"data": "overview"}}"#)
            .create_async()
            .await;

        let client = http::BunDocsClient::new_with_url(server.url());
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
    async fn test_handle_resources_read_http_error() {
        let mut server = mockito::Server::new_async().await;

        let _mock = server
            .mock("POST", "/")
            .with_status(500)
            .with_body("Internal Server Error")
            .create_async()
            .await;

        let client = http::BunDocsClient::new_with_url(server.url());
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: json!("res6"),
            method: "resources/read".to_string(),
            params: Some(json!({"uri": "bun://docs?query=test"})),
        };

        let response = handle_resources_read(&client, &request).await;
        let serialized = serde_json::to_value(&response).unwrap();

        assert!(serialized["error"].is_object());
        assert_eq!(serialized["error"]["code"], -32603);
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
    fn test_print_version() {
        // Test that print_version doesn't panic
        // Can't easily test output without mocking stdout
        let result = std::panic::catch_unwind(|| {
            print_version();
        });
        assert!(result.is_ok());
    }

    #[test]
    fn test_print_help() {
        // Test that print_help doesn't panic
        let result = std::panic::catch_unwind(|| {
            print_help();
        });
        assert!(result.is_ok());
    }

    #[test]
    fn test_handle_args_no_args() {
        // Test with no args (simulate program name only)
        let args = vec!["program".to_string()];
        let result = handle_args(&args);
        assert!(!result); // Should return false when no flags present
    }

    #[test]
    fn test_handle_args_help_flag() {
        // Test --help flag
        let args = vec!["program".to_string(), "--help".to_string()];
        let result = handle_args(&args);
        assert!(result); // Should return true for help flag
    }

    #[test]
    fn test_handle_args_help_short_flag() {
        // Test -h flag
        let args = vec!["program".to_string(), "-h".to_string()];
        let result = handle_args(&args);
        assert!(result); // Should return true for help flag
    }

    #[test]
    fn test_handle_args_version_flag() {
        // Test --version flag
        let args = vec!["program".to_string(), "--version".to_string()];
        let result = handle_args(&args);
        assert!(result); // Should return true for version flag
    }

    #[test]
    fn test_handle_args_version_short_flag() {
        // Test -V flag
        let args = vec!["program".to_string(), "-V".to_string()];
        let result = handle_args(&args);
        assert!(result); // Should return true for version flag
    }
}
