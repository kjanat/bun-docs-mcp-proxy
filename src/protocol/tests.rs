use super::*;
use serde_json::json;

#[test]
fn test_deserialize_jsonrpc_request() {
    let json_str = r#"{
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list",
            "params": {"query": "test"}
        }"#;

    let request: JsonRpcRequest = serde_json::from_str(json_str).unwrap();
    assert_eq!(request.jsonrpc, "2.0");
    assert_eq!(request.id, json!(1));
    assert_eq!(request.method, "tools/list");
    assert!(request.params.is_some());
}

#[test]
fn test_deserialize_jsonrpc_request_no_params() {
    let json_str = r#"{
            "jsonrpc": "2.0",
            "id": "test-id",
            "method": "initialize"
        }"#;

    let request: JsonRpcRequest = serde_json::from_str(json_str).unwrap();
    assert_eq!(request.method, "initialize");
    assert!(request.params.is_none());
}

#[test]
fn test_serialize_success_response() {
    let response = JsonRpcResponse::success(json!(1), json!({"status": "ok"}));
    let serialized = serde_json::to_value(&response).unwrap();

    assert_eq!(serialized["jsonrpc"], "2.0");
    assert_eq!(serialized["id"], 1);
    assert_eq!(serialized["result"]["status"], "ok");
    assert!(serialized.get("error").is_none());
}

#[test]
fn test_serialize_error_response() {
    let response = JsonRpcResponse::error(json!(1), -32700, "Parse error".to_owned());
    let serialized = serde_json::to_value(&response).unwrap();

    assert_eq!(serialized["jsonrpc"], "2.0");
    assert_eq!(serialized["id"], 1);
    assert_eq!(serialized["error"]["code"], -32700);
    assert_eq!(serialized["error"]["message"], "Parse error");
    assert!(serialized.get("result").is_none());
}

#[test]
fn test_error_response_without_data() {
    let response = JsonRpcResponse::error(json!(null), -32601, "Method not found".to_owned());
    let serialized = serde_json::to_string(&response).unwrap();

    // Verify data field is omitted when None
    assert!(!serialized.contains("\"data\""));
}

#[test]
fn test_jsonrpc_version_constant() {
    assert_eq!(JSONRPC_VERSION, "2.0");
}

#[test]
fn test_jsonrpc_error_new() {
    let error = JsonRpcError::new(-32700, "Parse error".to_owned());
    assert_eq!(error.code, -32700);
    assert_eq!(error.message, "Parse error");
    assert!(error.data.is_none());
}

#[test]
fn test_jsonrpc_error_with_data() {
    let data = json!({"details": "additional info"});
    let error = JsonRpcError::with_data(-32700, "Parse error".to_owned(), data.clone());
    assert_eq!(error.code, -32700);
    assert_eq!(error.message, "Parse error");
    assert_eq!(error.data, Some(data));
}

#[test]
fn test_error_response_with_data() {
    let data = json!({"reason": "invalid format"});
    let response =
        JsonRpcResponse::error_with_data(json!(1), -32700, "Parse error".to_owned(), data.clone());
    let serialized = serde_json::to_value(&response).unwrap();

    assert_eq!(serialized["jsonrpc"], "2.0");
    assert_eq!(serialized["id"], 1);
    assert_eq!(serialized["error"]["code"], -32700);
    assert_eq!(serialized["error"]["message"], "Parse error");
    assert_eq!(serialized["error"]["data"], data);
}
