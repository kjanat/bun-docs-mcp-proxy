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
    pub const fn new(code: i32, message: String) -> Self {
        Self {
            code,
            message,
            data: None,
        }
    }

    /// Create a new JSON-RPC error with additional data
    pub const fn with_data(code: i32, message: String, data: serde_json::Value) -> Self {
        Self {
            code,
            message,
            data: Some(data),
        }
    }
}

impl JsonRpcResponse {
    pub fn success(id: serde_json::Value, result: serde_json::Value) -> Self {
        return Self {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id,
            result: Some(result),
            error: None,
        };
    }

    pub fn error(id: serde_json::Value, code: i32, message: String) -> Self {
        return Self {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id,
            result: None,
            error: Some(JsonRpcError::new(code, message)),
        };
    }

    /// Create an error response with additional data
    pub fn error_with_data(
        id: serde_json::Value,
        code: i32,
        message: String,
        data: serde_json::Value,
    ) -> Self {
        return Self {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id,
            result: None,
            error: Some(JsonRpcError::with_data(code, message, data)),
        };
    }
}

#[cfg(test)]
mod tests;
