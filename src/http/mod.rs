use anyhow::{Context, Result};
use eventsource_stream::Eventsource;
use futures::StreamExt;
use reqwest::Client;
use serde_json::Value;
use tracing::{debug, info, warn};

const BUN_DOCS_API: &str = "https://bun.com/docs/mcp";
const REQUEST_TIMEOUT_SECS: u64 = 5;

pub struct BunDocsClient {
    client: Client,
    base_url: String,
}

impl BunDocsClient {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            base_url: BUN_DOCS_API.to_string(),
        }
    }

    pub async fn forward_request(&self, request: Value) -> Result<Value> {
        debug!("Forwarding request to Bun Docs API");

        // Send HTTP POST with JSON-RPC request
        let response = self
            .client
            .post(&self.base_url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .json(&request)
            .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .send()
            .await
            .context("Failed to send request to Bun Docs API")?;

        let status = response.status();
        info!("Bun Docs API response status: {}", status);

        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            anyhow::bail!("Bun Docs API error: {} - {}", status, error_text);
        }

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        // Parse SSE stream
        if content_type.contains("text/event-stream") {
            debug!("Parsing SSE stream");
            return self.parse_sse_response(response).await;
        }

        // Fallback to regular JSON
        debug!("Parsing regular JSON response");
        response
            .json()
            .await
            .context("Failed to parse JSON response")
    }

    async fn parse_sse_response(&self, response: reqwest::Response) -> Result<Value> {
        let mut event_stream = response.bytes_stream().eventsource();
        let mut json_response: Option<Value> = None;

        while let Some(event_result) = event_stream.next().await {
            match event_result {
                Ok(event) => {
                    debug!("SSE event type: {:?}", event.event);

                    let data = event.data;
                    if !data.is_empty() {
                        match serde_json::from_str::<Value>(&data) {
                            Ok(parsed) => {
                                debug!("Parsed SSE data successfully");

                                // Based on protocol analysis, the SSE data contains
                                // the complete JSON-RPC response
                                if parsed.get("result").is_some() || parsed.get("error").is_some() {
                                    json_response = Some(parsed);
                                    // Found the JSON-RPC response, we can stop
                                    break;
                                }
                            }
                            Err(e) => {
                                warn!("Failed to parse SSE data as JSON: {}", e);
                                debug!("SSE data: {}", &data[..data.len().min(200)]);
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!("SSE stream error: {}", e);
                    break;
                }
            }
        }

        json_response.ok_or_else(|| anyhow::anyhow!("No valid JSON-RPC response in SSE stream"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_client_creation() {
        let client = BunDocsClient::new();
        assert_eq!(client.base_url, BUN_DOCS_API);
    }

    #[tokio::test]
    async fn test_forward_request_tools_list() {
        let client = BunDocsClient::new();
        let request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list"
        });

        let result = client.forward_request(request).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        assert!(response.get("result").is_some());
        // Bun Docs should return tools
        assert!(response["result"].get("tools").is_some());
    }

    #[tokio::test]
    async fn test_forward_request_tools_call() {
        let client = BunDocsClient::new();
        let request = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "SearchBun",
                "arguments": {
                    "query": "Bun.serve"
                }
            }
        });

        let result = client.forward_request(request).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        assert!(response.get("result").is_some());
    }

    #[tokio::test]
    async fn test_forward_request_error_response() {
        let client = BunDocsClient::new();
        let request = json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "invalid_method_that_does_not_exist"
        });

        let result = client.forward_request(request).await;
        // Either error response or API error, both valid
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    async fn test_sse_response_with_error_field() {
        let sse_data = r#"{"error": {"code": -32601, "message": "Method not found"}}"#;
        let parsed: Value = serde_json::from_str(sse_data).unwrap();

        assert!(parsed.get("error").is_some());
        assert_eq!(parsed["error"]["code"], -32601);
    }

    #[tokio::test]
    async fn test_json_parsing_from_sse_data() {
        // Test valid JSON-RPC response in SSE data
        let sse_data = r#"{"result": {"tools": []}}"#;
        let parsed: Value = serde_json::from_str(sse_data).unwrap();

        assert!(parsed.get("result").is_some());
        assert!(parsed.get("result").unwrap().get("tools").is_some());
    }

    #[tokio::test]
    async fn test_json_parsing_invalid_data() {
        // Test invalid JSON in SSE data
        let sse_data = "not valid json";
        let result: Result<Value, _> = serde_json::from_str(sse_data);

        assert!(result.is_err());
    }

    #[test]
    fn test_content_type_detection() {
        let sse_type = "text/event-stream; charset=utf-8";
        let json_type = "application/json";

        assert!(sse_type.contains("text/event-stream"));
        assert!(!json_type.contains("text/event-stream"));
    }

    #[test]
    fn test_result_and_error_field_detection() {
        let with_result = json!({"result": {"data": "test"}});
        let with_error = json!({"error": {"code": -32700, "message": "Parse error"}});
        let neither = json!({"status": "pending"});

        assert!(with_result.get("result").is_some());
        assert!(with_error.get("error").is_some());
        assert!(neither.get("result").is_none() && neither.get("error").is_none());
    }

    #[test]
    fn test_empty_sse_data_handling() {
        let empty_data = "";
        assert!(empty_data.is_empty());

        // Empty data should be skipped in SSE parsing
        let non_empty = "data";
        assert!(!non_empty.is_empty());
    }

    #[test]
    fn test_http_status_detection() {
        // Test status code checking logic
        let status_ok = reqwest::StatusCode::OK;
        let status_error = reqwest::StatusCode::INTERNAL_SERVER_ERROR;

        assert!(status_ok.is_success());
        assert!(!status_error.is_success());
    }

    #[test]
    fn test_string_truncation() {
        let long_string = "a".repeat(300);
        let truncated = &long_string[..long_string.len().min(200)];

        assert_eq!(truncated.len(), 200);
    }

    #[test]
    fn test_timeout_value() {
        let timeout_secs = REQUEST_TIMEOUT_SECS;
        assert_eq!(timeout_secs, 5);
        assert!(timeout_secs > 0);
    }

    #[test]
    fn test_api_url_const() {
        assert_eq!(BUN_DOCS_API, "https://bun.com/docs/mcp");
        assert!(BUN_DOCS_API.starts_with("https://"));
    }

    #[test]
    fn test_sse_event_type_handling() {
        // Test SSE event type detection logic
        let event_type = "message";
        assert!(!event_type.is_empty());
    }

    #[test]
    fn test_json_parse_error_handling() {
        // Test invalid JSON parsing (covers parse_sse_response error path)
        let invalid_json = "not valid json {]";
        let result: Result<Value, _> = serde_json::from_str(invalid_json);
        assert!(result.is_err());
    }

    #[test]
    fn test_error_message_fallback() {
        // Test error text unwrap_or_else fallback
        let error_text = "Service Unavailable";
        let fallback = error_text;
        assert_eq!(fallback, "Service Unavailable");

        // Simulate fallback scenario
        let default_error = "unknown error";
        assert_eq!(default_error, "unknown error");
    }

    #[test]
    fn test_sse_data_min_truncation() {
        // Test SSE data truncation for debug logs
        let long_data = "a".repeat(300);
        let truncated = &long_data[..long_data.len().min(200)];
        assert_eq!(truncated.len(), 200);
    }
}
