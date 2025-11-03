//! JSON-RPC 2.0 protocol types and helpers
//!
//! This module provides type-safe representations of JSON-RPC 2.0 requests, responses,
//! and errors, following the [JSON-RPC 2.0 specification](https://www.jsonrpc.org/specification).
//!
//! ## Types
//!
//! - [`JsonRpcRequest`] - Incoming JSON-RPC request with method and optional params
//! - [`JsonRpcResponse`] - Outgoing JSON-RPC response with result or error
//! - [`JsonRpcError`] - Error object with code, message, and optional data
//!
//! ## Error Codes
//!
//! Standard JSON-RPC 2.0 error codes are defined in `src/main.rs`:
//! - `-32700` - Parse error (invalid JSON)
//! - `-32600` - Invalid request (malformed JSON-RPC)
//! - `-32601` - Method not found
//! - `-32602` - Invalid params
//! - `-32603` - Internal error
//!
//! ## Example Usage
//!
//! ```rust
//! use serde_json::json;
//! # use bun_docs_mcp_proxy::protocol::JsonRpcResponse;
//!
//! // Success response
//! let response = JsonRpcResponse::success(json!(1), json!({"result": "data"}));
//!
//! // Error response
//! let error = JsonRpcResponse::error(json!(1), -32601, "Method not found".to_string());
//! ```

use serde::{Deserialize, Serialize};

// JSON-RPC 2.0 version constant
const JSONRPC_VERSION: &str = "2.0";

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcError {
    /// Create a new JSON-RPC error without additional data
    pub fn new(code: i32, message: String) -> Self {
        Self {
            code,
            message,
            data: None,
        }
    }

    /// Create a new JSON-RPC error with additional data
    #[allow(dead_code)]
    pub fn with_data(code: i32, message: String, data: serde_json::Value) -> Self {
        Self {
            code,
            message,
            data: Some(data),
        }
    }
}

impl JsonRpcResponse {
    pub fn success(id: serde_json::Value, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: serde_json::Value, code: i32, message: String) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            result: None,
            error: Some(JsonRpcError::new(code, message)),
        }
    }

    /// Create an error response with additional data
    #[allow(dead_code)]
    pub fn error_with_data(
        id: serde_json::Value,
        code: i32,
        message: String,
        data: serde_json::Value,
    ) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            result: None,
            error: Some(JsonRpcError::with_data(code, message, data)),
        }
    }
}

#[cfg(test)]
mod tests {
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
        let response = JsonRpcResponse::error(json!(1), -32700, "Parse error".to_string());
        let serialized = serde_json::to_value(&response).unwrap();

        assert_eq!(serialized["jsonrpc"], "2.0");
        assert_eq!(serialized["id"], 1);
        assert_eq!(serialized["error"]["code"], -32700);
        assert_eq!(serialized["error"]["message"], "Parse error");
        assert!(serialized.get("result").is_none());
    }

    #[test]
    fn test_error_response_without_data() {
        let response = JsonRpcResponse::error(json!(null), -32601, "Method not found".to_string());
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
        let error = JsonRpcError::new(-32700, "Parse error".to_string());
        assert_eq!(error.code, -32700);
        assert_eq!(error.message, "Parse error");
        assert!(error.data.is_none());
    }

    #[test]
    fn test_jsonrpc_error_with_data() {
        let data = json!({"details": "additional info"});
        let error = JsonRpcError::with_data(-32700, "Parse error".to_string(), data.clone());
        assert_eq!(error.code, -32700);
        assert_eq!(error.message, "Parse error");
        assert_eq!(error.data, Some(data));
    }

    #[test]
    fn test_error_response_with_data() {
        let data = json!({"reason": "invalid format"});
        let response = JsonRpcResponse::error_with_data(
            json!(1),
            -32700,
            "Parse error".to_string(),
            data.clone(),
        );
        let serialized = serde_json::to_value(&response).unwrap();

        assert_eq!(serialized["jsonrpc"], "2.0");
        assert_eq!(serialized["id"], 1);
        assert_eq!(serialized["error"]["code"], -32700);
        assert_eq!(serialized["error"]["message"], "Parse error");
        assert_eq!(serialized["error"]["data"], data);
    }
}
