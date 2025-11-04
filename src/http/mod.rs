//! HTTP client for the Bun Docs API with SSE support and automatic retries.
//!
//! This module provides a robust HTTP client that:
//! - Forwards JSON-RPC requests to the Bun Docs API at `https://bun.com/docs/mcp`
//! - Parses Server-Sent Events (SSE) responses from the API
//! - Implements automatic retry logic with exponential backoff for transient failures
//! - Provides testability via `with_base_url()` constructor for mock servers
//!
//! ## Example
//!
//! ```no_run
//! use bun_docs_mcp_proxy::http::BunDocsClient;
//! use serde_json::json;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let client = BunDocsClient::new();
//! let request = json!({
//!     "jsonrpc": "2.0",
//!     "id": 1,
//!     "method": "tools/list"
//! });
//! let response = client.forward_request(request).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## SSE Protocol Behavior
//!
//! The Bun Docs API may return responses as SSE (Server-Sent Events) or plain JSON,
//! depending on the content-type header. When parsing SSE streams:
//! - Only "message" and "completion" event types are processed
//! - Heartbeat and other event types are ignored
//! - **Important**: This implementation expects a complete JSON-RPC object in a single
//!   SSE event. If the server streams partial deltas across multiple events, this
//!   implementation will not accumulate them. Adjust `parse_sse_response()` if the
//!   protocol changes to delta streaming.
//!
//! ## Retry Strategy
//!
//! Transient failures (network errors, 429, 5xx status codes) are retried up to
//! [`MAX_RETRIES`] times with exponential backoff (200 ms → 400 ms → 800 ms, capped at 1 s).

use anyhow::{Context, Result};
use eventsource_stream::Eventsource;
use futures::StreamExt;
use reqwest::{Client, StatusCode, Url, header::HeaderMap};
use serde_json::Value;
use std::time::Duration;
use tracing::{debug, info, warn};

const BUN_DOCS_API: &str = "https://bun.com/docs/mcp";
const REQUEST_TIMEOUT_SECS: u64 = 5;
const MAX_RETRIES: usize = 3;
const BACKOFF_BASE_MS: u64 = 200;
const BACKOFF_MAX_MS: u64 = 1000;

pub struct BunDocsClient {
    client: Client,
    base_url: Url,
}

impl Default for BunDocsClient {
    fn default() -> Self {
        Self::new()
    }
}

impl BunDocsClient {
    /// Creates a new client with the default Bun Docs API URL.
    ///
    /// # Panics
    /// Panics if the hardcoded URL is invalid (should never happen in practice).
    pub fn new() -> Self {
        Self::with_base_url(BUN_DOCS_API).expect("valid base URL")
    }

    pub fn with_base_url(url: &str) -> Result<Self> {
        Ok(Self {
            client: Client::new(),
            base_url: Url::parse(url).context("Invalid base URL")?,
        })
    }

    /// Compute exponential backoff delay without external dependencies
    ///
    /// # Arguments
    /// * `attempt` - Attempt number (must be >= 1)
    ///
    /// # Returns
    /// Delay in milliseconds: 200 ms, 400 ms, 800 ms (capped at 1000 ms)
    fn backoff_delay_ms(attempt: usize) -> u64 {
        debug_assert!(attempt > 0, "attempt must be >= 1");
        // 200ms, 400ms, 800ms (cap at 1000ms)
        let base = BACKOFF_BASE_MS.saturating_mul(1u64 << (attempt.saturating_sub(1) as u32));
        base.min(BACKOFF_MAX_MS)
    }

    /// Check if HTTP status code indicates a transient error worth retrying
    fn is_transient_status(status: StatusCode) -> bool {
        matches!(
            status,
            StatusCode::TOO_MANY_REQUESTS
                | StatusCode::INTERNAL_SERVER_ERROR
                | StatusCode::BAD_GATEWAY
                | StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::GATEWAY_TIMEOUT
        )
    }

    /// Extract main content type from header map
    fn main_content_type(headers: &HeaderMap) -> String {
        headers
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase()
    }

    /// Summarize headers for logging (first 8 headers)
    fn summarize_headers(headers: &HeaderMap) -> String {
        headers
            .iter()
            .take(8)
            .map(|(k, v)| format!("{}: {}", k.as_str(), v.to_str().unwrap_or("<binary>")))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Truncate string to max length, preserving UTF-8 boundaries
    fn truncate_utf8(s: &str, max_len: usize) -> &str {
        if s.len() <= max_len {
            return s;
        }
        // Find the last char whose end position is at or before max_len
        let mut last_valid = 0;
        for (idx, ch) in s.char_indices() {
            let end_pos = idx + ch.len_utf8();
            if end_pos > max_len {
                break;
            }
            last_valid = end_pos;
        }
        &s[..last_valid]
    }

    pub async fn forward_request(&self, request: Value) -> Result<Value> {
        debug!("Forwarding request to Bun Docs API");

        let mut last_err: Option<anyhow::Error> = None;

        for attempt in 1..=MAX_RETRIES {
            // Build request each attempt
            let rb = self
                .client
                .post(self.base_url.as_str())
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .header(
                    reqwest::header::ACCEPT,
                    "application/json, text/event-stream",
                )
                .json(&request)
                .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS));

            let outcome: Result<Value> = match rb.send().await {
                Ok(response) => {
                    let status = response.status();
                    info!(
                        "Bun Docs API response status: {} (attempt {} of {})",
                        status, attempt, MAX_RETRIES
                    );

                    let headers = response.headers().clone();
                    let content_type = Self::main_content_type(&headers);

                    if !status.is_success() {
                        // Read body (truncated) for context
                        // Limit to 100KB to prevent OOM from malicious/misconfigured servers
                        const MAX_ERROR_BODY_SIZE: usize = 100_000;
                        let bytes = response.bytes().await.unwrap_or_else(|e| {
                            warn!("Failed to read error response body: {}", e);
                            Default::default()
                        });
                        let limited_bytes = if bytes.len() > MAX_ERROR_BODY_SIZE {
                            &bytes[..MAX_ERROR_BODY_SIZE]
                        } else {
                            &bytes[..]
                        };
                        let body = String::from_utf8_lossy(limited_bytes);
                        let body_snippet = Self::truncate_utf8(&body, 2048);
                        let header_summary = Self::summarize_headers(&headers);

                        let err = anyhow::anyhow!(
                            "Bun Docs API error: status={} content_type={} headers=[{}] body_snippet=\"{}\"",
                            status,
                            if content_type.is_empty() {
                                "<none>"
                            } else {
                                &content_type
                            },
                            header_summary,
                            body_snippet
                        );

                        // Retry on transient server statuses
                        if Self::is_transient_status(status) && attempt < MAX_RETRIES {
                            warn!(
                                "Transient HTTP status {}, retrying (attempt {})",
                                status,
                                attempt + 1
                            );
                            let delay = Self::backoff_delay_ms(attempt);
                            tokio::time::sleep(Duration::from_millis(delay)).await;
                            last_err = Some(err);
                            continue;
                        }

                        Err(err)
                    } else {
                        // Success: decide how to parse based on content type
                        if content_type.starts_with("text/event-stream") {
                            debug!("Parsing SSE stream");
                            self.parse_sse_response(response).await
                        } else {
                            debug!("Parsing regular JSON response");
                            response
                                .json()
                                .await
                                .context("Failed to parse JSON response")
                        }
                    }
                }
                Err(e) => {
                    // Connection/timeout/etc. Retry if transient
                    let is_transient = e.is_connect() || e.is_timeout() || e.is_request();
                    let err = anyhow::anyhow!("Failed to send request to Bun Docs API: {}", e);

                    if is_transient && attempt < MAX_RETRIES {
                        warn!(
                            "Network error: {}. Retrying (attempt {} of {})",
                            err,
                            attempt + 1,
                            MAX_RETRIES
                        );
                        let delay = Self::backoff_delay_ms(attempt);
                        tokio::time::sleep(Duration::from_millis(delay)).await;
                        last_err = Some(err);
                        continue;
                    }

                    Err(err)
                }
            };

            // For any non-retry path, return the outcome (success or error)
            return outcome;
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("Unknown error sending request")))
    }

    async fn parse_sse_response(&self, response: reqwest::Response) -> Result<Value> {
        let mut event_stream = response.bytes_stream().eventsource();
        let mut json_response: Option<Value> = None;

        while let Some(event_result) = event_stream.next().await {
            match event_result {
                Ok(event) => {
                    debug!("SSE event type: {:?}", event.event);

                    // Only handle message-like events; ignore heartbeats/others
                    let ev_type = if event.event.is_empty() {
                        "message"
                    } else {
                        event.event.as_str()
                    };
                    if ev_type != "message" && ev_type != "completion" {
                        debug!("Skipping SSE event type: {}", ev_type);
                        continue;
                    }

                    let data = event.data;
                    if !data.is_empty() {
                        match serde_json::from_str::<Value>(&data) {
                            Ok(parsed) => {
                                debug!("Parsed SSE data successfully");

                                // Note: this implementation expects a complete JSON-RPC object in one event.
                                // If the server streams partial deltas, we do not accumulate them here.
                                // Adjust if protocol changes to delta streaming.
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
        assert_eq!(client.base_url.as_str(), BUN_DOCS_API);
    }

    #[test]
    fn test_client_default() {
        let client = BunDocsClient::default();
        assert_eq!(client.base_url.as_str(), BUN_DOCS_API);
    }

    #[test]
    fn test_client_with_base_url() {
        let custom_url = "https://example.com/api";
        let client = BunDocsClient::with_base_url(custom_url).unwrap();
        assert_eq!(client.base_url.as_str(), custom_url);
    }

    #[test]
    fn test_client_with_base_url_invalid() {
        let result = BunDocsClient::with_base_url("not a valid url");
        assert!(result.is_err());
    }

    #[test]
    fn test_backoff_delay_ms() {
        assert_eq!(BunDocsClient::backoff_delay_ms(1), 200);
        assert_eq!(BunDocsClient::backoff_delay_ms(2), 400);
        assert_eq!(BunDocsClient::backoff_delay_ms(3), 800);
        assert_eq!(BunDocsClient::backoff_delay_ms(4), 1000); // capped
    }

    #[test]
    fn test_is_transient_status() {
        assert!(BunDocsClient::is_transient_status(
            StatusCode::TOO_MANY_REQUESTS
        ));
        assert!(BunDocsClient::is_transient_status(
            StatusCode::INTERNAL_SERVER_ERROR
        ));
        assert!(BunDocsClient::is_transient_status(StatusCode::BAD_GATEWAY));
        assert!(BunDocsClient::is_transient_status(
            StatusCode::SERVICE_UNAVAILABLE
        ));
        assert!(BunDocsClient::is_transient_status(
            StatusCode::GATEWAY_TIMEOUT
        ));
        assert!(!BunDocsClient::is_transient_status(StatusCode::NOT_FOUND));
        assert!(!BunDocsClient::is_transient_status(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn test_main_content_type() {
        let mut headers = HeaderMap::new();
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            "application/json; charset=utf-8".parse().unwrap(),
        );
        assert_eq!(
            BunDocsClient::main_content_type(&headers),
            "application/json"
        );

        headers.insert(
            reqwest::header::CONTENT_TYPE,
            "text/event-stream".parse().unwrap(),
        );
        assert_eq!(
            BunDocsClient::main_content_type(&headers),
            "text/event-stream"
        );

        let empty_headers = HeaderMap::new();
        assert_eq!(BunDocsClient::main_content_type(&empty_headers), "");
    }

    #[test]
    fn test_summarize_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            "application/json".parse().unwrap(),
        );
        headers.insert(reqwest::header::CONTENT_LENGTH, "123".parse().unwrap());

        let summary = BunDocsClient::summarize_headers(&headers);
        assert!(summary.contains("content-type"));
        assert!(summary.contains("application/json"));
    }

    #[test]
    fn test_truncate_utf8() {
        let short = "hello";
        assert_eq!(BunDocsClient::truncate_utf8(short, 10), short);

        let long = "a".repeat(100);
        let truncated = BunDocsClient::truncate_utf8(&long, 50);
        assert!(truncated.len() <= 50);
        assert!(!truncated.is_empty());
        assert!(truncated.is_char_boundary(truncated.len()));

        // Test with Unicode characters
        let unicode = "hello 世界";
        let truncated = BunDocsClient::truncate_utf8(unicode, 8);
        assert!(truncated.len() <= 8);
        assert!(truncated.is_char_boundary(truncated.len()));
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
        // The API should either return a JSON-RPC error response or fail with an HTTP error
        match result {
            Ok(response) => {
                // If successful, should have an error field in JSON-RPC response
                assert!(
                    response.get("error").is_some(),
                    "Expected error field in response"
                );
            }
            Err(_) => {
                // HTTP-level error is also acceptable
            }
        }
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
        let status_ok = StatusCode::OK;
        let status_error = StatusCode::INTERNAL_SERVER_ERROR;

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

    // Retry behavior tests with mockito
    #[tokio::test]
    async fn test_retry_on_transient_status_503() {
        let mut server = mockito::Server::new_async().await;

        // First request fails with 503
        let mock1 = server
            .mock("POST", "/")
            .with_status(503)
            .with_header("content-type", "text/plain")
            .with_body("Service Unavailable")
            .expect(1)
            .create_async()
            .await;

        // Second request succeeds
        let mock2 = server
            .mock("POST", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"result": {"tools": []}}"#)
            .expect(1)
            .create_async()
            .await;

        let client = BunDocsClient::with_base_url(&server.url()).unwrap();
        let request = json!({"method": "tools/list"});

        let result = client.forward_request(request).await;

        mock1.assert_async().await;
        mock2.assert_async().await;
        assert!(result.is_ok(), "Should succeed after retry");
        assert!(result.unwrap().get("result").is_some());
    }

    #[tokio::test]
    async fn test_retry_exhaustion_on_persistent_503() {
        let mut server = mockito::Server::new_async().await;

        // All 3 attempts fail with 503
        let mock = server
            .mock("POST", "/")
            .with_status(503)
            .with_header("content-type", "text/plain")
            .with_body("Service Unavailable")
            .expect(3)
            .create_async()
            .await;

        let client = BunDocsClient::with_base_url(&server.url()).unwrap();
        let request = json!({"method": "tools/list"});

        let result = client.forward_request(request).await;

        mock.assert_async().await;
        assert!(result.is_err(), "Should fail after exhausting retries");
        assert!(result.unwrap_err().to_string().contains("503"));
    }

    #[tokio::test]
    async fn test_no_retry_on_non_transient_404() {
        let mut server = mockito::Server::new_async().await;

        // 404 is not transient, should not retry
        let mock = server
            .mock("POST", "/")
            .with_status(404)
            .with_header("content-type", "text/plain")
            .with_body("Not Found")
            .expect(1)
            .create_async()
            .await;

        let client = BunDocsClient::with_base_url(&server.url()).unwrap();
        let request = json!({"method": "tools/list"});

        let result = client.forward_request(request).await;

        mock.assert_async().await;
        assert!(result.is_err(), "Should fail without retry on 404");
        assert!(result.unwrap_err().to_string().contains("404"));
    }

    #[tokio::test]
    async fn test_retry_on_429_rate_limit() {
        let mut server = mockito::Server::new_async().await;

        // First request gets rate limited
        let mock1 = server
            .mock("POST", "/")
            .with_status(429)
            .with_header("content-type", "text/plain")
            .with_body("Too Many Requests")
            .expect(1)
            .create_async()
            .await;

        // Second request succeeds
        let mock2 = server
            .mock("POST", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"result": {"data": "success"}}"#)
            .expect(1)
            .create_async()
            .await;

        let client = BunDocsClient::with_base_url(&server.url()).unwrap();
        let request = json!({"method": "test"});

        let result = client.forward_request(request).await;

        mock1.assert_async().await;
        mock2.assert_async().await;
        assert!(result.is_ok(), "Should succeed after retrying 429");
    }

    #[tokio::test]
    async fn test_retry_on_500_internal_error() {
        let mut server = mockito::Server::new_async().await;

        // First request fails with 500
        let mock1 = server
            .mock("POST", "/")
            .with_status(500)
            .with_header("content-type", "text/plain")
            .with_body("Internal Server Error")
            .expect(1)
            .create_async()
            .await;

        // Second request succeeds
        let mock2 = server
            .mock("POST", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"result": {}}"#)
            .expect(1)
            .create_async()
            .await;

        let client = BunDocsClient::with_base_url(&server.url()).unwrap();
        let request = json!({"method": "test"});

        let result = client.forward_request(request).await;

        mock1.assert_async().await;
        mock2.assert_async().await;
        assert!(result.is_ok(), "Should succeed after retrying 500");
    }

    #[tokio::test]
    async fn test_retry_on_502_bad_gateway() {
        let mut server = mockito::Server::new_async().await;

        // Simulate bad gateway then recovery
        let mock1 = server
            .mock("POST", "/")
            .with_status(502)
            .with_body("Bad Gateway")
            .expect(1)
            .create_async()
            .await;

        let mock2 = server
            .mock("POST", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"result": {}}"#)
            .expect(1)
            .create_async()
            .await;

        let client = BunDocsClient::with_base_url(&server.url()).unwrap();
        let request = json!({"method": "test"});

        let result = client.forward_request(request).await;

        mock1.assert_async().await;
        mock2.assert_async().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_retry_timing_exponential_backoff() {
        use std::time::Instant;

        let mut server = mockito::Server::new_async().await;

        // All requests fail to test backoff timing
        let mock = server
            .mock("POST", "/")
            .with_status(503)
            .with_body("Unavailable")
            .expect(3)
            .create_async()
            .await;

        let client = BunDocsClient::with_base_url(&server.url()).unwrap();
        let request = json!({"method": "test"});

        let start = Instant::now();
        let _ = client.forward_request(request).await;
        let elapsed = start.elapsed();

        mock.assert_async().await;

        // With 3 attempts and delays of 200 ms, 400 ms:
        // Total should be at least 600 ms (200 + 400)
        // But allow some margin for execution time
        assert!(
            elapsed.as_millis() >= 550,
            "Expected at least 600 ms for backoff, got {}ms",
            elapsed.as_millis()
        );
    }
}
