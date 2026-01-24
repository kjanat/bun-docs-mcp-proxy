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
use serde_json::Value;

/// The fixed JSON-RPC 2.0 protocol version string.
const JSONRPC_VERSION: &str = "2.0";

/// JSON-RPC 2.0 request structure
///
/// Per JSON-RPC 2.0 spec:
/// - Requests have an `id` field (string, number, or null)
/// - Notifications omit the `id` field entirely (no response expected)
#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    /// Protocol version (must be "2.0")
    #[allow(dead_code, reason = "field required for protocol compliance")]
    pub jsonrpc: String,
    /// Request identifier (can be string, number, or null)
    /// None indicates a notification (no response expected per JSON-RPC 2.0)
    #[serde(default)]
    pub id: Option<Value>,
    /// Method name to invoke
    pub method: String,
    /// Optional method parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// JSON-RPC 2.0 response structure
#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    /// Protocol version (always "2.0")
    pub jsonrpc: String,
    /// Request identifier (matches the request id)
    pub id: Value,
    /// Successful result (mutually exclusive with error)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Error object (mutually exclusive with result)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC 2.0 error object
#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    /// Error code (standard codes are negative)
    pub code: i32,
    /// Human-readable error message
    pub message: String,
    /// Optional additional error data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// The error object within a JSON-RPC error response envelope.
///
/// This is used for parsing upstream responses where we need to preserve
/// the original error structure including the i64 code.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JsonRpcErrorObject {
    /// Error code (i64 to handle any upstream value)
    pub code: i64,
    /// Human-readable error message
    pub message: String,
    /// Optional additional error data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Represents a parsed JSON-RPC 2.0 response envelope.
///
/// Used for parsing upstream responses before converting to our response type.
/// The untagged enum allows serde to try both variants during deserialization.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcEnvelope {
    /// A successful JSON-RPC response with a result field
    Success {
        /// Protocol version (should be "2.0")
        #[allow(dead_code, reason = "required for serde deserialization")]
        jsonrpc: String,
        /// The result payload
        result: Value,
        /// Request identifier
        #[allow(dead_code, reason = "required for serde deserialization")]
        id: Value,
    },
    /// An error JSON-RPC response with an error field
    Error {
        /// Protocol version (should be "2.0")
        #[allow(dead_code, reason = "required for serde deserialization")]
        jsonrpc: String,
        /// The error object
        error: JsonRpcErrorObject,
        /// Request identifier
        #[allow(dead_code, reason = "required for serde deserialization")]
        id: Value,
    },
}

impl JsonRpcEnvelope {
    /// Try to parse a Value as a JSON-RPC envelope
    ///
    /// # Errors
    /// Returns an error if the value doesn't match the expected envelope structure
    pub fn from_value(value: Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value(value)
    }

    /// Returns true if this is a success response
    #[must_use]
    #[allow(dead_code, reason = "public API for consumers and tests")]
    pub const fn is_success(&self) -> bool {
        matches!(self, Self::Success { .. })
    }

    /// Returns true if this is an error response
    #[must_use]
    #[allow(dead_code, reason = "public API for consumers and tests")]
    pub const fn is_error(&self) -> bool {
        matches!(self, Self::Error { .. })
    }

    /// Extract the result value if success, None otherwise
    #[must_use]
    #[allow(dead_code, reason = "public API for consumers and tests")]
    pub const fn result(&self) -> Option<&Value> {
        match self {
            Self::Success { result, .. } => Some(result),
            Self::Error { .. } => None,
        }
    }

    /// Extract the error object if error, None otherwise
    #[must_use]
    #[allow(dead_code, reason = "public API for consumers and tests")]
    pub const fn error(&self) -> Option<&JsonRpcErrorObject> {
        match self {
            Self::Success { .. } => None,
            Self::Error { error, .. } => Some(error),
        }
    }

    /// Consume self and return the result value if success, None otherwise
    #[must_use]
    #[allow(dead_code, reason = "public API for consumers and tests")]
    pub fn into_result(self) -> Option<Value> {
        match self {
            Self::Success { result, .. } => Some(result),
            Self::Error { .. } => None,
        }
    }

    /// Consume self and return the error object if error, None otherwise
    #[must_use]
    #[allow(dead_code, reason = "public API for consumers and tests")]
    pub fn into_error(self) -> Option<JsonRpcErrorObject> {
        match self {
            Self::Success { .. } => None,
            Self::Error { error, .. } => Some(error),
        }
    }
}

impl JsonRpcError {
    /// Create a new JSON-RPC error without additional data
    ///
    /// # Arguments
    /// * `code` - Error code (standard JSON-RPC codes are negative)
    /// * `message` - Human-readable error message
    ///
    /// # Returns
    /// New `JsonRpcError` instance without additional data
    #[must_use]
    pub const fn new(code: i32, message: String) -> Self {
        Self {
            code,
            message,
            data: None,
        }
    }

    /// Create a new JSON-RPC error with additional data
    ///
    /// # Arguments
    /// * `code` - Error code (standard JSON-RPC codes are negative)
    /// * `message` - Human-readable error message
    /// * `data` - Additional structured error information
    ///
    /// # Returns
    /// New `JsonRpcError` instance with additional data
    #[must_use]
    #[allow(dead_code, reason = "reserved for protocol compliance")]
    pub const fn with_data(code: i32, message: String, data: Value) -> Self {
        Self {
            code,
            message,
            data: Some(data),
        }
    }
}

impl JsonRpcResponse {
    /// Create a successful JSON-RPC response
    ///
    /// # Arguments
    /// * `id` - Request identifier to match
    /// * `result` - Successful result value
    ///
    /// # Returns
    /// New `JsonRpcResponse` with result field populated
    #[must_use]
    pub fn success(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Create an error JSON-RPC response
    ///
    /// # Arguments
    /// * `id` - Request identifier to match
    /// * `code` - Error code
    /// * `message` - Error message
    ///
    /// # Returns
    /// New `JsonRpcResponse` with error field populated
    #[must_use]
    pub fn error(id: Value, code: i32, message: String) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id,
            result: None,
            error: Some(JsonRpcError::new(code, message)),
        }
    }

    /// Create an error response with additional data
    ///
    /// # Arguments
    /// * `id` - Request identifier to match
    /// * `code` - Error code
    /// * `message` - Error message
    /// * `data` - Additional error information
    ///
    /// # Returns
    /// New `JsonRpcResponse` with error field and additional data
    #[must_use]
    #[allow(dead_code, reason = "reserved for protocol compliance")]
    pub fn error_with_data(id: Value, code: i32, message: String, data: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id,
            result: None,
            error: Some(JsonRpcError::with_data(code, message, data)),
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "tests can use expect()")]
#[allow(clippy::unwrap_used, reason = "tests can use unwrap()")]
#[allow(clippy::indexing_slicing, reason = "tests use array indexing")]
#[allow(clippy::default_numeric_fallback, reason = "test literals")]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserialize_jsonrpc_request() {
        let json_str = r#"{
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list",
            "params": {"query": "test"}
        }"#;

        let request: JsonRpcRequest =
            serde_json::from_str(json_str).expect("valid JSON-RPC request should parse");
        assert_eq!(request.jsonrpc, "2.0");
        assert_eq!(request.id, Some(json!(1_i32)));
        assert_eq!(request.method, "tools/list");
        assert!(request.params.is_some());
    }

    #[test]
    fn deserialize_jsonrpc_request_no_params() {
        let json_str = r#"{
            "jsonrpc": "2.0",
            "id": "test-id",
            "method": "initialize"
        }"#;

        let request: JsonRpcRequest =
            serde_json::from_str(json_str).expect("valid JSON-RPC request should parse");
        assert_eq!(request.method, "initialize");
        assert!(request.params.is_none());
    }

    #[test]
    fn deserialize_jsonrpc_notification() {
        // JSON-RPC 2.0 notifications have no id field
        let json_str = r#"{
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }"#;

        let request: JsonRpcRequest =
            serde_json::from_str(json_str).expect("valid JSON-RPC notification should parse");
        assert_eq!(request.jsonrpc, "2.0");
        assert!(request.id.is_none(), "notifications should have no id");
        assert_eq!(request.method, "notifications/initialized");
        assert!(request.params.is_none());
    }

    #[test]
    fn deserialize_jsonrpc_notification_with_params() {
        // JSON-RPC 2.0 notifications can have params
        let json_str = r#"{
            "jsonrpc": "2.0",
            "method": "notifications/progress",
            "params": {"progress": 50}
        }"#;

        let request: JsonRpcRequest =
            serde_json::from_str(json_str).expect("valid JSON-RPC notification should parse");
        assert!(request.id.is_none(), "notifications should have no id");
        assert_eq!(request.method, "notifications/progress");
        assert!(request.params.is_some());
    }

    #[test]
    fn serialize_success_response() {
        let response = JsonRpcResponse::success(json!(1_i32), json!({"status": "ok"}));
        let serialized =
            serde_json::to_value(&response).expect("response should serialize to JSON");

        let jsonrpc_field = serialized
            .get("jsonrpc")
            .expect("jsonrpc field should exist");
        assert_eq!(jsonrpc_field, "2.0");

        let id_field = serialized.get("id").expect("id field should exist");
        assert_eq!(id_field, &json!(1_i32));

        let result_field = serialized.get("result").expect("result field should exist");
        let status_field = result_field
            .get("status")
            .expect("status field should exist");
        assert_eq!(status_field, "ok");

        assert!(serialized.get("error").is_none());
    }

    #[test]
    fn serialize_error_response() {
        let response = JsonRpcResponse::error(json!(1_i32), -32_700_i32, "Parse error".to_owned());
        let serialized =
            serde_json::to_value(&response).expect("response should serialize to JSON");

        let jsonrpc_field = serialized
            .get("jsonrpc")
            .expect("jsonrpc field should exist");
        assert_eq!(jsonrpc_field, "2.0");

        let id_field = serialized.get("id").expect("id field should exist");
        assert_eq!(id_field, &json!(1_i32));

        let error_field = serialized.get("error").expect("error field should exist");
        let code_field = error_field.get("code").expect("code field should exist");
        assert_eq!(code_field, &json!(-32_700_i32));

        let message_field = error_field
            .get("message")
            .expect("message field should exist");
        assert_eq!(message_field, "Parse error");

        assert!(serialized.get("result").is_none());
    }

    #[test]
    fn error_response_without_data() {
        let response =
            JsonRpcResponse::error(json!(null), -32_601_i32, "Method not found".to_owned());
        let serialized = serde_json::to_string(&response).expect("response should serialize");

        // Verify data field is omitted when None
        assert!(!serialized.contains("\"data\""));
    }

    #[test]
    fn jsonrpc_version_constant() {
        assert_eq!(JSONRPC_VERSION, "2.0");
    }

    #[test]
    fn jsonrpc_error_new() {
        let error = JsonRpcError::new(-32_700_i32, "Parse error".to_owned());
        assert_eq!(error.code, -32_700_i32);
        assert_eq!(error.message, "Parse error");
        assert!(error.data.is_none());
    }

    #[test]
    fn jsonrpc_error_with_data() {
        let data = json!({"details": "additional info"});
        let error = JsonRpcError::with_data(-32_700_i32, "Parse error".to_owned(), data.clone());
        assert_eq!(error.code, -32_700_i32);
        assert_eq!(error.message, "Parse error");
        assert_eq!(error.data, Some(data));
    }

    #[test]
    fn error_response_with_data() {
        let data = json!({"reason": "invalid format"});
        let response = JsonRpcResponse::error_with_data(
            json!(1_i32),
            -32_700_i32,
            "Parse error".to_owned(),
            data.clone(),
        );
        let serialized =
            serde_json::to_value(&response).expect("response should serialize to JSON");

        let jsonrpc_field = serialized
            .get("jsonrpc")
            .expect("jsonrpc field should exist");
        assert_eq!(jsonrpc_field, "2.0");

        let id_field = serialized.get("id").expect("id field should exist");
        assert_eq!(id_field, &json!(1_i32));

        let error_field = serialized.get("error").expect("error field should exist");
        let code_field = error_field.get("code").expect("code field should exist");
        assert_eq!(code_field, &json!(-32_700_i32));

        let message_field = error_field
            .get("message")
            .expect("message field should exist");
        assert_eq!(message_field, "Parse error");

        let data_field = error_field.get("data").expect("data field should exist");
        assert_eq!(data_field, &data);
    }

    // JsonRpcEnvelope tests

    #[test]
    fn parse_success_envelope() {
        let json = r#"{"jsonrpc":"2.0","result":{"foo":"bar"},"id":1}"#;
        let envelope: JsonRpcEnvelope = serde_json::from_str(json).unwrap();
        assert!(envelope.is_success());
        assert!(!envelope.is_error());
        assert!(envelope.result().is_some());
        assert!(envelope.error().is_none());
        assert_eq!(envelope.result().unwrap(), &json!({"foo": "bar"}));
    }

    #[test]
    fn parse_error_envelope() {
        let json = r#"{"jsonrpc":"2.0","error":{"code":-32600,"message":"Invalid"},"id":1}"#;
        let envelope: JsonRpcEnvelope = serde_json::from_str(json).unwrap();
        assert!(envelope.is_error());
        assert!(!envelope.is_success());
        assert!(envelope.error().is_some());
        assert!(envelope.result().is_none());
        assert_eq!(envelope.error().unwrap().code, -32600);
        assert_eq!(envelope.error().unwrap().message, "Invalid");
    }

    #[test]
    fn parse_error_envelope_with_data() {
        let json = r#"{"jsonrpc":"2.0","error":{"code":-32000,"message":"Custom","data":{"detail":"info"}},"id":"abc"}"#;
        let envelope: JsonRpcEnvelope = serde_json::from_str(json).unwrap();
        assert!(envelope.is_error());
        let error = envelope.error().unwrap();
        assert_eq!(error.code, -32000);
        assert_eq!(error.message, "Custom");
        assert!(error.data.is_some());
        assert_eq!(error.data.as_ref().unwrap(), &json!({"detail": "info"}));
    }

    #[test]
    fn envelope_from_value_success() {
        let value = json!({"jsonrpc": "2.0", "result": [1, 2, 3], "id": 42});
        let envelope = JsonRpcEnvelope::from_value(value).unwrap();
        assert!(envelope.is_success());
        assert_eq!(envelope.result().unwrap(), &json!([1, 2, 3]));
    }

    #[test]
    fn envelope_from_value_error() {
        let value = json!({
            "jsonrpc": "2.0",
            "error": {"code": -32601, "message": "Method not found"},
            "id": null
        });
        let envelope = JsonRpcEnvelope::from_value(value).unwrap();
        assert!(envelope.is_error());
        assert_eq!(envelope.error().unwrap().code, -32601);
    }

    #[test]
    fn envelope_from_value_invalid() {
        // Missing required fields
        let value = json!({"foo": "bar"});
        JsonRpcEnvelope::from_value(value).unwrap_err();
    }

    #[test]
    fn error_object_serialization() {
        let error_obj = JsonRpcErrorObject {
            code: -32700,
            message: "Parse error".to_owned(),
            data: Some(json!({"line": 10})),
        };
        let serialized = serde_json::to_value(&error_obj).unwrap();
        assert_eq!(serialized["code"], -32700);
        assert_eq!(serialized["message"], "Parse error");
        assert_eq!(serialized["data"]["line"], 10);
    }

    #[test]
    fn error_object_without_data_omits_field() {
        let error_obj = JsonRpcErrorObject {
            code: -32600,
            message: "Invalid".to_owned(),
            data: None,
        };
        let serialized = serde_json::to_string(&error_obj).unwrap();
        assert!(!serialized.contains("\"data\""));
    }

    #[test]
    fn envelope_into_result_success() {
        let envelope: JsonRpcEnvelope =
            serde_json::from_str(r#"{"jsonrpc":"2.0","result":{"data":"test"},"id":1}"#).unwrap();
        let result = envelope.into_result();
        assert!(result.is_some());
        assert_eq!(result.unwrap(), json!({"data": "test"}));
    }

    #[test]
    fn envelope_into_result_error_returns_none() {
        let envelope: JsonRpcEnvelope = serde_json::from_str(
            r#"{"jsonrpc":"2.0","error":{"code":-32600,"message":"Invalid"},"id":1}"#,
        )
        .unwrap();
        let result = envelope.into_result();
        assert!(result.is_none());
    }

    #[test]
    fn envelope_into_error_success_returns_none() {
        let envelope: JsonRpcEnvelope =
            serde_json::from_str(r#"{"jsonrpc":"2.0","result":"ok","id":1}"#).unwrap();
        let error = envelope.into_error();
        assert!(error.is_none());
    }

    #[test]
    fn envelope_into_error_error() {
        let envelope: JsonRpcEnvelope = serde_json::from_str(
            r#"{"jsonrpc":"2.0","error":{"code":-32601,"message":"Not found"},"id":1}"#,
        )
        .unwrap();
        let error = envelope.into_error();
        assert!(error.is_some());
        let err = error.unwrap();
        assert_eq!(err.code, -32601);
        assert_eq!(err.message, "Not found");
    }
}
