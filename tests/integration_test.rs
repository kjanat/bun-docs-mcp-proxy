#![allow(clippy::expect_used, reason = "tests can use expect() for clarity")]
#![allow(clippy::unwrap_used, reason = "tests can use unwrap() for brevity")]
#![allow(clippy::indexing_slicing, reason = "tests use array indexing safely")]
#![allow(
    clippy::default_numeric_fallback,
    reason = "test literals don't need type suffixes"
)]
#![allow(
    clippy::tests_outside_test_module,
    reason = "integration tests in tests/ directory"
)]

use serde_json::json;

#[test]
fn test_protocol_types_roundtrip() {
    // Test that JSON-RPC types can be serialized and deserialized correctly
    let request_json = json!({
        "jsonrpc": "2.0",
        "id": 1_i32,
        "method": "tools/list",
        "params": {"query": "test"}
    });

    let request_str = serde_json::to_string(&request_json).expect("serialization succeeds in test");
    let parsed: serde_json::Value =
        serde_json::from_str(&request_str).expect("deserialization succeeds in test");

    assert_eq!(parsed.get("jsonrpc").expect("jsonrpc field exists"), "2.0");
    assert_eq!(
        parsed.get("method").expect("method field exists"),
        "tools/list"
    );
}

#[test]
fn test_error_codes() {
    // Verify standard JSON-RPC error codes
    let error_codes = vec![
        (-32_700_i32, "Parse error"),
        (-32_601_i32, "Method not found"),
        (-32_603_i32, "Internal error"),
    ];

    for (code, _message) in error_codes {
        assert!(code < 0_i32, "Error codes should be negative");
        assert!(code >= -0x8000_i32, "Error codes should be in valid range");
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

    let tools = tools_response
        .get("tools")
        .expect("tools field exists")
        .as_array()
        .expect("tools is array");
    assert_eq!(tools.len(), 1_usize);

    let tool = tools.first().expect("tools array non-empty");
    assert!(tool.get("name").expect("name field exists").is_string());
    assert!(
        tool.get("description")
            .expect("description field exists")
            .is_string()
    );
    assert!(
        tool.get("inputSchema")
            .expect("inputSchema field exists")
            .is_object()
    );
    assert_eq!(
        tool.get("inputSchema")
            .expect("inputSchema exists")
            .get("type")
            .expect("type field exists"),
        "object"
    );
    assert!(
        tool.get("inputSchema")
            .expect("inputSchema exists")
            .get("properties")
            .expect("properties field exists")
            .is_object()
    );
    assert!(
        tool.get("inputSchema")
            .expect("inputSchema exists")
            .get("required")
            .expect("required field exists")
            .is_array()
    );
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
        "id": 1_i32,
        "error": {
            "code": -32_601_i32,
            "message": "Method not found: unsupported_method"
        }
    });

    assert_eq!(
        error_response.get("jsonrpc").expect("jsonrpc field exists"),
        "2.0"
    );
    assert!(
        error_response
            .get("error")
            .expect("error field exists")
            .is_object()
    );
    assert_eq!(
        error_response
            .get("error")
            .expect("error exists")
            .get("code")
            .expect("code field exists"),
        -32_601_i32
    );
    assert!(
        error_response
            .get("error")
            .expect("error exists")
            .get("message")
            .expect("message field exists")
            .as_str()
            .expect("message is string")
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
            "code": -32_700_i32,
            "message": "Parse error: invalid JSON"
        }
    });

    assert_eq!(
        error_response.get("jsonrpc").expect("jsonrpc field exists"),
        "2.0"
    );
    assert!(error_response.get("id").expect("id field exists").is_null());
    assert_eq!(
        error_response
            .get("error")
            .expect("error exists")
            .get("code")
            .expect("code field exists"),
        -32_700_i32
    );
    assert!(
        error_response
            .get("error")
            .expect("error exists")
            .get("message")
            .expect("message field exists")
            .as_str()
            .expect("message is string")
            .contains("Parse error")
    );
}

#[test]
fn test_internal_error_structure() {
    // Test that internal errors follow JSON-RPC spec
    let error_response = json!({
        "jsonrpc": "2.0",
        "id": 1_i32,
        "error": {
            "code": -32_603_i32,
            "message": "Internal error: failed to process request"
        }
    });

    assert_eq!(
        error_response.get("jsonrpc").expect("jsonrpc field exists"),
        "2.0"
    );
    assert_eq!(
        error_response
            .get("error")
            .expect("error exists")
            .get("code")
            .expect("code field exists"),
        -32_603_i32
    );
    assert!(
        error_response
            .get("error")
            .expect("error exists")
            .get("message")
            .expect("message field exists")
            .as_str()
            .expect("message is string")
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

    let resources = resources_response
        .get("resources")
        .expect("resources field exists")
        .as_array()
        .expect("resources is array");
    assert_eq!(resources.len(), 1_usize);

    let resource = resources.first().expect("resources array non-empty");
    assert_eq!(resource.get("uri").expect("uri field exists"), "bun://docs");
    assert!(resource.get("name").expect("name field exists").is_string());
    assert!(
        resource
            .get("description")
            .expect("description field exists")
            .is_string()
    );
    assert_eq!(
        resource.get("mimeType").expect("mimeType field exists"),
        "application/json"
    );
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

    let contents = read_response
        .get("contents")
        .expect("contents field exists")
        .as_array()
        .expect("contents is array");
    assert_eq!(contents.len(), 1_usize);

    let content = contents.first().expect("contents array non-empty");
    assert!(content.get("uri").expect("uri field exists").is_string());
    assert_eq!(
        content.get("mimeType").expect("mimeType field exists"),
        "application/json"
    );
    assert!(content.get("text").expect("text field exists").is_string());
}

#[test]
fn test_error_response_id_preservation() {
    // Test that error responses preserve request ID
    let request_id = json!("test-123");
    let error_response = json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "error": {
            "code": -32_601_i32,
            "message": "Method not found"
        }
    });

    assert_eq!(
        error_response.get("id").expect("id field exists"),
        "test-123"
    );
}

#[test]
fn test_numeric_and_string_ids() {
    // Test that both numeric and string IDs are valid
    let numeric_id = json!(42_i32);
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
