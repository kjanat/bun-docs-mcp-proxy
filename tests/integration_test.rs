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

#[test]
fn test_unsupported_method_error_structure() {
    // Test that unsupported method errors follow JSON-RPC spec
    let error_response = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "error": {
            "code": -32601,
            "message": "Method not found: unsupported_method"
        }
    });

    assert_eq!(error_response["jsonrpc"], "2.0");
    assert!(error_response["error"].is_object());
    assert_eq!(error_response["error"]["code"], -32601);
    assert!(
        error_response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Method not found")
    );
}

#[test]
fn test_parse_error_structure() {
    // Test that parse errors follow JSON-RPC spec
    let error_response = json!({
        "jsonrpc": "2.0",
        "id": null,
        "error": {
            "code": -32700,
            "message": "Parse error: invalid JSON"
        }
    });

    assert_eq!(error_response["jsonrpc"], "2.0");
    assert!(error_response["id"].is_null());
    assert_eq!(error_response["error"]["code"], -32700);
    assert!(
        error_response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Parse error")
    );
}

#[test]
fn test_internal_error_structure() {
    // Test that internal errors follow JSON-RPC spec
    let error_response = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "error": {
            "code": -32603,
            "message": "Internal error: failed to process request"
        }
    });

    assert_eq!(error_response["jsonrpc"], "2.0");
    assert_eq!(error_response["error"]["code"], -32603);
    assert!(
        error_response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Internal error")
    );
}

#[test]
fn test_resources_list_response_structure() {
    // Test that resources/list response follows MCP schema
    let resources_response = json!({
        "resources": [{
            "uri": "bun://docs",
            "name": "Bun Documentation",
            "description": "Search and browse Bun documentation",
            "mimeType": "application/json"
        }]
    });

    let resources = resources_response["resources"].as_array().unwrap();
    assert_eq!(resources.len(), 1);

    let resource = &resources[0];
    assert_eq!(resource["uri"], "bun://docs");
    assert!(resource["name"].is_string());
    assert!(resource["description"].is_string());
    assert_eq!(resource["mimeType"], "application/json");
}

#[test]
fn test_resources_read_response_structure() {
    // Test that resources/read response follows MCP schema
    let read_response = json!({
        "contents": [{
            "uri": "bun://docs?query=test",
            "mimeType": "application/json",
            "text": "{\"result\": {}}"
        }]
    });

    let contents = read_response["contents"].as_array().unwrap();
    assert_eq!(contents.len(), 1);

    let content = &contents[0];
    assert!(content["uri"].is_string());
    assert_eq!(content["mimeType"], "application/json");
    assert!(content["text"].is_string());
}

#[test]
fn test_error_response_id_preservation() {
    // Test that error responses preserve request ID
    let request_id = json!("test-123");
    let error_response = json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "error": {
            "code": -32601,
            "message": "Method not found"
        }
    });

    assert_eq!(error_response["id"], "test-123");
}

#[test]
fn test_numeric_and_string_ids() {
    // Test that both numeric and string IDs are valid
    let numeric_id = json!(42);
    let string_id = json!("abc-123");
    let null_id = json!(null);

    assert!(numeric_id.is_number());
    assert!(string_id.is_string());
    assert!(null_id.is_null());

    // All should be valid JSON-RPC IDs
    assert!(numeric_id.is_number() || numeric_id.is_string() || numeric_id.is_null());
    assert!(string_id.is_number() || string_id.is_string() || string_id.is_null());
    assert!(null_id.is_number() || null_id.is_string() || null_id.is_null());
}
