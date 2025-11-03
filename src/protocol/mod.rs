use serde::{Deserialize, Serialize};

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

impl JsonRpcResponse {
    pub fn success(id: serde_json::Value, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: serde_json::Value, code: i32, message: String) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message,
                data: None,
            }),
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
}
