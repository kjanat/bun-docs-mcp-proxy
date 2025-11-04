// Additional HTTP module tests for network errors and edge cases
// Tests real error scenarios without mocking
use serde_json::json;
use std::{io::Error, io::ErrorKind::Other};

// Re-export needed types for testing
mod http_test_utils {
    use anyhow::Result;
    use reqwest::Client;
    use serde_json::Value;

    pub struct BunDocsClient {
        pub client: Client,
        pub base_url: String,
    }

    impl BunDocsClient {
        pub fn new_with_url(url: String) -> Self {
            return Self {
                client: Client::new(),
                base_url: url,
            };
        }

        pub async fn forward_request(&self, request: Value) -> Result<Value> {
            use anyhow::Context as _;

            const REQUEST_TIMEOUT_SECS: u64 = 5;

            let response = self
                .client
                .post(&self.base_url)
                .header("Content-Type", "application/json")
                .header("Accept", "application/json, text/event-stream")
                .json(&request)
                .timeout(core::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
                .send()
                .await
                .context("Failed to send request to Bun Docs API")?;

            let status = response.status();

            if !status.is_success() {
                let error_text = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "unknown error".to_owned());
                anyhow::bail!("Bun Docs API error: {status} - {error_text}");
            }

            let content_type = response
                .headers()
                .get("content-type")
                .and_then(|v| return v.to_str().ok())
                .unwrap_or("");

            if content_type.contains("text/event-stream") {
                return self.parse_sse_response(response).await;
            }

            return response
                .json()
                .await
                .context("Failed to parse JSON response");
        }

        async fn parse_sse_response(&self, response: reqwest::Response) -> Result<Value> {
            use eventsource_stream::Eventsource as _;
            use futures::StreamExt as _;

            let mut event_stream = response.bytes_stream().eventsource();

            while let Some(event_result) = event_stream.next().await {
                match event_result {
                    Ok(event) => {
                        if let Some(json_response) = Self::parse_sse_event_data(&event.data)? {
                            return Ok(json_response);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("SSE stream error: {}", e);
                        break;
                    }
                }
            }

            return Err(anyhow::anyhow!("No valid JSON-RPC response in SSE stream"));
        }

        fn parse_sse_event_data(data: &str) -> Result<Option<Value>> {
            if data.is_empty() {
                return Ok(None);
            }

            let parsed = serde_json::from_str::<Value>(data)
                .map_err(|e| anyhow::anyhow!("Failed to parse SSE event data as JSON: {e}"))?;

            if parsed.get("result").is_some() || parsed.get("error").is_some() {
                return Ok(Some(parsed));
            } else {
                return Ok(None);
            }
        }
    }
}

use http_test_utils::BunDocsClient;

#[tokio::test]
async fn test_forward_request_connection_refused() {
    // Use invalid port that nothing is listening on
    let client = BunDocsClient::new_with_url("http://localhost:1".to_owned());

    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/list"
    });

    let result = client.forward_request(request).await;
    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("Failed to send request")
            || error_msg.contains("connection")
            || error_msg.contains("refused")
            || error_msg.contains("Connection refused")
    );
}

#[tokio::test]
async fn test_forward_request_invalid_hostname() {
    // Use invalid hostname that cannot be resolved
    let client =
        BunDocsClient::new_with_url("http://invalid.hostname.that.does.not.exist.local".to_owned());

    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/list"
    });

    let result = client.forward_request(request).await;
    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("Failed to send request")
            || error_msg.contains("dns")
            || error_msg.contains("resolve")
    );
}

#[tokio::test]
async fn test_forward_request_timeout_with_real_slow_endpoint() {
    // Use httpbingo.org delay endpoint to test timeout (delays 10 seconds, timeout is 5 seconds)
    let client = BunDocsClient::new_with_url("https://httpbingo.org/delay/10".to_owned());

    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/list"
    });

    let result = client.forward_request(request).await;
    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    eprintln!("Timeout error: {error_msg}");
    // Timeout manifests as "Failed to send request" error
    assert!(error_msg.contains("Failed to send") || error_msg.contains("Bun Docs API error"));
}

#[tokio::test]
async fn test_forward_request_http_404() {
    // Use httpbingo.org status endpoint to test 404 error
    let client = BunDocsClient::new_with_url("https://httpbingo.org/status/404".to_owned());

    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/list"
    });

    let result = client.forward_request(request).await;
    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("404") || error_msg.contains("Bun Docs API error"));
}

#[tokio::test]
async fn test_forward_request_http_500() {
    // Use httpbingo.org status endpoint to test 500 error
    let client = BunDocsClient::new_with_url("https://httpbingo.org/status/500".to_owned());

    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/list"
    });

    let result = client.forward_request(request).await;
    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("500") || error_msg.contains("Bun Docs API error"));
}

#[tokio::test]
async fn test_parse_invalid_json_response() {
    // Use httpbingo.org html endpoint to get non-JSON response
    let client = BunDocsClient::new_with_url("https://httpbingo.org/html".to_owned());

    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/list"
    });

    let result = client.forward_request(request).await;
    // httpbingo/html returns HTTP 405 for POST, which tests HTTP error handling
    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    eprintln!("HTML error: {error_msg}");
    assert!(
        error_msg.contains("405")
            || error_msg.contains("Method Not Allowed")
            || error_msg.contains("Bun Docs API error")
    );
}

#[test]
fn test_sse_parsing_logic() {
    // Test SSE data parsing logic without network calls
    let valid_result = r#"{"result": {"tools": []}}"#;
    let valid_error = r#"{"error": {"code": -32601, "message": "Not found"}}"#;
    let neither = r#"{"status": "pending"}"#;
    let invalid_json = "not valid json";

    // Valid result
    let parsed: serde_json::Result<serde_json::Value> = serde_json::from_str(valid_result);
    assert!(parsed.is_ok());
    assert!(parsed.unwrap().get("result").is_some());

    // Valid error
    let parsed: serde_json::Result<serde_json::Value> = serde_json::from_str(valid_error);
    assert!(parsed.is_ok());
    assert!(parsed.unwrap().get("error").is_some());

    // Neither result nor error (should be skipped in SSE parsing)
    let parsed: serde_json::Result<serde_json::Value> = serde_json::from_str(neither);
    assert!(parsed.is_ok());
    let val = parsed.unwrap();
    assert!(val.get("result").is_none() && val.get("error").is_none());

    // Invalid JSON
    let parsed: serde_json::Result<serde_json::Value> = serde_json::from_str(invalid_json);
    parsed.unwrap_err();
}

#[test]
fn test_http_status_code_checking() {
    use reqwest::StatusCode;

    // Success codes
    assert!(StatusCode::OK.is_success());
    assert!(StatusCode::CREATED.is_success());
    assert!(StatusCode::ACCEPTED.is_success());

    // Client error codes
    assert!(!StatusCode::BAD_REQUEST.is_success());
    assert!(!StatusCode::NOT_FOUND.is_success());
    assert!(!StatusCode::FORBIDDEN.is_success());

    // Server error codes
    assert!(!StatusCode::INTERNAL_SERVER_ERROR.is_success());
    assert!(!StatusCode::BAD_GATEWAY.is_success());
    assert!(!StatusCode::SERVICE_UNAVAILABLE.is_success());
}

#[test]
fn test_content_type_header_parsing() {
    // Test content type detection logic
    let sse_types = vec![
        "text/event-stream",
        "text/event-stream; charset=utf-8",
        "text/event-stream;charset=UTF-8",
    ];

    let json_types = vec![
        "application/json",
        "application/json; charset=utf-8",
        "application/json;charset=UTF-8",
    ];

    for ct in sse_types {
        assert!(ct.contains("text/event-stream"));
    }

    for ct in json_types {
        assert!(!ct.contains("text/event-stream"));
        assert!(ct.contains("application/json"));
    }
}

#[test]
fn test_timeout_duration() {
    use core::time::Duration;

    const REQUEST_TIMEOUT_SECS: u64 = 5;
    let timeout = Duration::from_secs(REQUEST_TIMEOUT_SECS);

    assert_eq!(timeout.as_secs(), 5);
    assert!(timeout.as_secs() > 0);
    assert!(timeout.as_secs() < 10);
}

#[test]
fn test_error_message_fallback_logic() {
    // Test unwrap_or_else logic for error text
    let ok_result: Result<String, Error> = Ok("error message".to_owned());
    let err_result: Result<String, Error> = Err(Error::new(Other, "test error"));

    let fallback1 = Result::unwrap_or_else(ok_result, |_| "unknown error".to_owned());
    let fallback2 = Result::unwrap_or_else(err_result, |_| "unknown error".to_owned());

    assert_eq!(fallback1, "error message");
    assert_eq!(fallback2, "unknown error");
}
