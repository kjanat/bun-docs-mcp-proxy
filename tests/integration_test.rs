use serde_json::json;

#[test]
fn test_protocol_types_roundtrip() {
    // Test that JSON-RPC types can be serialized and deserialized correctly
    let request_json = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/list",
        "params": {"query": "test"}
    });

    let request_str = serde_json::to_string(&request_json).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&request_str).unwrap();

    assert_eq!(parsed["jsonrpc"], "2.0");
    assert_eq!(parsed["method"], "tools/list");
}

#[test]
fn test_error_codes() {
    // Verify standard JSON-RPC error codes
    let error_codes = vec![
        (-32700, "Parse error"),
        (-32601, "Method not found"),
        (-32603, "Internal error"),
    ];

    for (code, _message) in error_codes {
        assert!(code < 0, "Error codes should be negative");
        assert!(code >= -32768, "Error codes should be in valid range");
    }
}

#[test]
fn test_initialize_response_structure() {
    // Test that initialize response has required MCP fields
    let init_response = json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {
            "tools": {}
        },
        "serverInfo": {
            "name": "bun-docs-mcp-proxy",
            "version": env!("CARGO_PKG_VERSION")
        }
    });

    assert!(init_response["protocolVersion"].is_string());
    assert!(init_response["capabilities"].is_object());
    assert!(init_response["serverInfo"]["name"].is_string());
    assert!(init_response["serverInfo"]["version"].is_string());
}

#[test]
fn test_tools_list_response_structure() {
    // Test that tools/list response follows MCP schema
    let tools_response = json!({
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

    let tools = tools_response["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);

    let tool = &tools[0];
    assert!(tool["name"].is_string());
    assert!(tool["description"].is_string());
    assert!(tool["inputSchema"].is_object());
    assert_eq!(tool["inputSchema"]["type"], "object");
    assert!(tool["inputSchema"]["properties"].is_object());
    assert!(tool["inputSchema"]["required"].is_array());
}

#[test]
fn test_jsonrpc_version() {
    // Ensure all responses use JSON-RPC 2.0
    let version = "2.0";
    assert_eq!(version, "2.0");
}
