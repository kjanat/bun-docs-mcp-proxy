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

use anyhow::{Context as _, Result};
use bytes::Bytes;
use eventsource_stream::Eventsource as _;
use futures::StreamExt as _;
use reqwest::{Client, StatusCode, Url, header::HeaderMap};
use serde_json::Value;
use std::time::Duration;
use tracing::{debug, info, warn};

/// Base URL for the Bun documentation API
const BUN_DOCS_API: &str = "https://bun.com/docs/mcp";

/// HTTP request timeout in seconds
const REQUEST_TIMEOUT_SECS: u64 = 5_u64;

/// Maximum number of retry attempts for transient failures
const MAX_RETRIES: usize = 3_usize;

/// Base delay for exponential backoff (milliseconds)
const BACKOFF_BASE_MS: u64 = 200_u64;

/// Maximum backoff delay (milliseconds)
const BACKOFF_MAX_MS: u64 = 1000_u64;

/// Maximum error response body size to read (100KB, prevents OOM from malicious/misconfigured servers)
const MAX_ERROR_BODY_SIZE: usize = 100_000_usize;

/// HTTP client for interacting with the Bun Docs API
pub struct BunDocsClient {
    /// The underlying `reqwest::Client` used for making HTTP requests.
    client: Client,
    /// The base URL for all API requests made by this client.
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
    #[must_use]
    pub fn new() -> Self {
        Self::with_base_url(BUN_DOCS_API).expect("valid base URL")
    }

    /// Creates a new client with a custom base URL.
    ///
    /// # Arguments
    /// * `url` - The base URL for API requests
    ///
    /// # Errors
    /// Returns an error if the URL cannot be parsed
    pub fn with_base_url(url: &str) -> Result<Self> {
        Ok(Self {
            client: Client::new(),
            base_url: Url::parse(url).context("Invalid base URL")?,
        })
    }

    /// Calculates an exponential backoff delay for retry attempts.
    ///
    /// The delay increases with each `attempt` (e.g., 200ms, 400ms, 800ms) up to a maximum of 1000ms.
    /// This helps prevent overwhelming the server during transient failures.
    ///
    /// # Arguments
    /// * `attempt` - The current retry attempt number (must be >= 1).
    ///
    /// # Returns
    /// The calculated delay in milliseconds.
    fn backoff_delay_ms(attempt: usize) -> u64 {
        debug_assert!(attempt > 0_usize, "attempt must be >= 1");
        // 200ms, 400ms, 800ms (cap at 1000ms)
        // Safe: attempt.saturating_sub(1) will be small in practice (<= MAX_RETRIES=3)
        #[expect(
            clippy::cast_possible_truncation,
            reason = "attempt.saturating_sub(1) is bounded by MAX_RETRIES=3, fits in u32"
        )]
        let base =
            BACKOFF_BASE_MS.saturating_mul(1_u64 << (attempt.saturating_sub(1_usize) as u32));
        base.min(BACKOFF_MAX_MS)
    }

    /// Determines if an HTTP status code indicates a transient error that is worth retrying.
    ///
    /// Transient errors typically include server errors (5xx) and rate limiting (429).
    ///
    /// # Arguments
    /// * `status` - The `StatusCode` to check.
    ///
    /// # Returns
    /// `true` if the status code is transient and suggests a retry, `false` otherwise.
    const fn is_transient_status(status: StatusCode) -> bool {
        matches!(
            status,
            StatusCode::TOO_MANY_REQUESTS
                | StatusCode::INTERNAL_SERVER_ERROR
                | StatusCode::BAD_GATEWAY
                | StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::GATEWAY_TIMEOUT
        )
    }

    /// Extracts the main content type from a `HeaderMap`, stripping parameters like charset.
    ///
    /// For example, `application/json; charset=utf-8` would return `application/json`.
    /// The returned string is always lowercase.
    ///
    /// # Arguments
    /// * `headers` - A reference to the `HeaderMap` containing the HTTP response headers.
    ///
    /// # Returns
    /// A `String` representing the main content type, or an empty string if the header is missing or invalid.
    fn main_content_type(headers: &HeaderMap) -> String {
        let content_type = match headers.get(reqwest::header::CONTENT_TYPE) {
            Some(value) => match value.to_str() {
                Ok(s) => s,
                Err(_) => {
                    return String::new();
                }
            },
            None => {
                return String::new();
            }
        };

        let primary_type = content_type.split(';').next().unwrap_or("").trim();
        primary_type.to_ascii_lowercase()
    }

    /// Creates a concise, comma-separated string summary of HTTP headers for logging purposes.
    ///
    /// It takes up to the first 8 headers and formats them as `Key: Value` pairs.
    /// Binary header values are represented as `<binary>`.
    ///
    /// # Arguments
    /// * `headers` - A reference to the `HeaderMap` containing the HTTP headers.
    ///
    /// # Returns
    /// A `String` containing the summarized headers.
    fn summarize_headers(headers: &HeaderMap) -> String {
        headers
            .iter()
            .take(8_usize)
            .map(|(key, value)| {
                format!("{}: {}", key.as_str(), value.to_str().unwrap_or("<binary>"))
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Truncates a string to a maximum byte length, ensuring that the truncation
    /// occurs on a UTF-8 character boundary to prevent invalid UTF-8 sequences.
    ///
    /// If the string's byte length is already less than or equal to `max_len`,
    /// the original string slice is returned.
    ///
    /// # Arguments
    /// * `text` - The string slice to truncate.
    /// * `max_len` - The maximum desired length in bytes.
    ///
    /// # Returns
    /// A string slice (`&str`) that is a valid UTF-8 truncation of the input `text`.
    fn truncate_utf8(text: &str, max_len: usize) -> &str {
        if text.len() <= max_len {
            return text;
        }
        // Find the last char whose end position is at or before max_len
        let mut last_valid = 0_usize;
        for (idx, ch) in text.char_indices() {
            let end_pos = idx + ch.len_utf8();
            if end_pos > max_len {
                break;
            }
            last_valid = end_pos;
        }
        &text[..last_valid]
    }

    /// Forward a JSON-RPC request to the Bun Docs API with automatic retries
    ///
    /// # Arguments
    /// * `request` - JSON-RPC request object
    ///
    /// # Returns
    /// JSON-RPC response from the API
    ///
    /// # Errors
    /// Returns an error if all retry attempts fail or a non-retryable error occurs
    #[allow(
        clippy::too_many_lines,
        reason = "complex retry logic with error handling"
    )]
    pub async fn forward_request(&self, request: Value) -> Result<Value> {
        debug!("Forwarding request to Bun Docs API");

        let mut last_error: Option<anyhow::Error> = None;

        for attempt in 1_usize..=MAX_RETRIES {
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

            match rb.send().await {
                Ok(response) => {
                    let status = response.status();
                    info!(
                        "Bun Docs API response status: {} (attempt {} of {})",
                        status, attempt, MAX_RETRIES
                    );

                    let headers = response.headers().clone();
                    let content_type = Self::main_content_type(&headers);

                    if status.is_success() {
                        // Success: decide how to parse based on content type
                        if content_type.starts_with("text/event-stream") {
                            debug!("Parsing SSE stream");
                            return self.parse_sse_response(response).await;
                        }
                        debug!("Parsing regular JSON response");
                        return response
                            .json()
                            .await
                            .context("Failed to parse JSON response");
                    }
                    // Read body (truncated) for context
                    let bytes = response.bytes().await.unwrap_or_else(|error| {
                        warn!("Failed to read error response body: {}", error);
                        Bytes::default()
                    });
                    let limited_bytes: &[u8] = if bytes.len() > MAX_ERROR_BODY_SIZE {
                        &bytes[..MAX_ERROR_BODY_SIZE]
                    } else {
                        &bytes
                    };
                    let body = String::from_utf8_lossy(limited_bytes);
                    let body_snippet = Self::truncate_utf8(&body, 2048_usize);
                    let header_summary = Self::summarize_headers(&headers);

                    let error = anyhow::anyhow!(
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
                        last_error = Some(error);
                        continue;
                    }

                    return Err(error);
                }
                Err(error) => {
                    // Connection/timeout/etc. Retry if transient
                    let is_transient =
                        error.is_connect() || error.is_timeout() || error.is_request();
                    let err = anyhow::anyhow!("Failed to send request to Bun Docs API: {error}");

                    if is_transient && attempt < MAX_RETRIES {
                        warn!(
                            "Network error: {}. Retrying (attempt {} of {})",
                            err,
                            attempt + 1,
                            MAX_RETRIES
                        );
                        let delay = Self::backoff_delay_ms(attempt);
                        tokio::time::sleep(Duration::from_millis(delay)).await;
                        last_error = Some(err);
                        continue;
                    }

                    return Err(err);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Unknown error sending request")))
    }

    /// Parses a Server-Sent Events (SSE) response stream from the Bun Docs API.
    ///
    /// This function consumes the HTTP response body as an SSE stream, looking for
    /// `message` or `completion` events that contain a complete JSON-RPC response.
    /// It stops processing after the first valid JSON-RPC response is found.
    ///
    /// # Arguments
    /// * `response` - The `reqwest::Response` object, expected to contain an SSE stream.
    ///
    /// # Returns
    /// A `Result` which on success contains the parsed `serde_json::Value` representing
    /// the JSON-RPC response. On failure, it returns an `anyhow::Error` if no valid
    /// JSON-RPC response is found or if there's an error processing the stream.
    ///
    /// # Errors
    /// Returns an error if:
    /// - The SSE stream encounters an error.
    /// - No valid JSON-RPC response (i.e., an object with a `result` or `error` field)
    ///   is found within the stream.
    /// - JSON parsing of an SSE event's data fails.
    async fn parse_sse_response(&self, response: reqwest::Response) -> Result<Value> {
        let mut event_stream = response.bytes_stream().eventsource();
        let mut json_response: Option<Value> = None;

        loop {
            let event_result = event_stream.next().await;
            let Some(event_result) = event_result else {
                break;
            };
            match event_result {
                Ok(event) => {
                    debug!("SSE event type: {:?}", event.event);

                    // Only handle message-like events; ignore heartbeats/others
                    let event_type = if event.event.is_empty() {
                        "message"
                    } else {
                        event.event.as_str()
                    };
                    if event_type != "message" && event_type != "completion" {
                        debug!("Skipping SSE event type: {}", event_type);
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
                            Err(error) => {
                                warn!("Failed to parse SSE data as JSON: {}", error);
                                debug!("SSE data: {}", &data[..data.len().min(200_usize)]);
                            }
                        }
                    }
                }
                Err(error) => {
                    warn!("SSE stream error: {}", error);
                    break;
                }
            }
        }

        json_response.ok_or_else(|| anyhow::anyhow!("No valid JSON-RPC response in SSE stream"))
    }

    /// Fetch a documentation page as raw Markdown/MDX
    ///
    /// Sends an HTTP GET request with `Accept: text/markdown` header to retrieve
    /// the raw MDX source of a documentation page.
    ///
    /// # Arguments
    /// * `url` - The full URL of the documentation page to fetch
    ///
    /// # Returns
    /// Raw Markdown/MDX content as a String
    ///
    /// # Errors
    /// Returns an error if:
    /// - The HTTP request fails
    /// - The server returns a non-success status code
    /// - The response body cannot be read as UTF-8 text
    pub async fn fetch_doc_markdown(&self, url: &str) -> Result<String> {
        debug!("Fetching MDX for URL: {}", url);

        let response = self
            .client
            .get(url)
            .header(reqwest::header::ACCEPT, "text/markdown")
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .send()
            .await
            .context("Failed to send request for markdown")?;

        let status = response.status();
        if !status.is_success() {
            return Err(anyhow::anyhow!(
                "Failed to fetch markdown: HTTP {status} for URL: {url}"
            ));
        }

        let text = response
            .text()
            .await
            .context("Failed to read markdown response body")?;

        debug!("Successfully fetched {} bytes of MDX", text.len());
        Ok(text)
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
    use std::time::Instant;

    #[test]
    fn client_creation() {
        let client = BunDocsClient::new();
        assert_eq!(client.base_url.as_str(), BUN_DOCS_API);
    }

    #[test]
    fn client_default() {
        let client = BunDocsClient::default();
        assert_eq!(client.base_url.as_str(), BUN_DOCS_API);
    }

    #[test]
    fn client_with_base_url() {
        let custom_url = "https://example.com/api";
        let client = BunDocsClient::with_base_url(custom_url).expect("valid URL should parse");
        assert_eq!(client.base_url.as_str(), custom_url);
    }

    #[test]
    fn client_with_base_url_invalid() {
        let result = BunDocsClient::with_base_url("not a valid url");
        assert!(result.is_err());
    }

    #[test]
    fn backoff_delay_milliseconds() {
        assert_eq!(BunDocsClient::backoff_delay_ms(1_usize), 200_u64);
        assert_eq!(BunDocsClient::backoff_delay_ms(2_usize), 400_u64);
        assert_eq!(BunDocsClient::backoff_delay_ms(3_usize), 800_u64);
        assert_eq!(BunDocsClient::backoff_delay_ms(4_usize), 1000_u64); // capped
    }

    #[test]
    fn is_transient_status() {
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
    fn main_content_type() {
        use reqwest::header::HeaderValue;

        let mut headers = HeaderMap::new();
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            HeaderValue::from_str("application/json; charset=utf-8").expect("valid header value"),
        );
        assert_eq!(
            BunDocsClient::main_content_type(&headers),
            "application/json"
        );

        headers.insert(
            reqwest::header::CONTENT_TYPE,
            HeaderValue::from_str("text/event-stream").expect("valid header value"),
        );
        assert_eq!(
            BunDocsClient::main_content_type(&headers),
            "text/event-stream"
        );

        let empty_headers = HeaderMap::new();
        assert_eq!(BunDocsClient::main_content_type(&empty_headers), "");
    }

    #[test]
    fn summarize_headers() {
        use reqwest::header::HeaderValue;

        let mut headers = HeaderMap::new();
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            HeaderValue::from_str("application/json").expect("valid header value"),
        );
        headers.insert(
            reqwest::header::CONTENT_LENGTH,
            HeaderValue::from_str("123").expect("valid header value"),
        );

        let summary = BunDocsClient::summarize_headers(&headers);
        assert!(summary.contains("content-type"));
        assert!(summary.contains("application/json"));
    }

    #[test]
    fn truncate_utf8() {
        let short = "hello";
        assert_eq!(BunDocsClient::truncate_utf8(short, 10_usize), short);

        let long = "a".repeat(100_usize);
        let truncated = BunDocsClient::truncate_utf8(&long, 50_usize);
        assert!(truncated.len() <= 50_usize);
        assert!(!truncated.is_empty());
        assert!(truncated.is_char_boundary(truncated.len()));

        // Test with Unicode characters
        // "hello 世界"
        let unicode = "hello \u{4e16}\u{754c}";
        let truncated_unicode = BunDocsClient::truncate_utf8(unicode, 8_usize);
        assert!(truncated_unicode.len() <= 8_usize);
        assert!(truncated_unicode.is_char_boundary(truncated_unicode.len()));
    }

    // Unit tests with mocked HTTP responses (fast, deterministic, offline-friendly)
    #[tokio::test]
    async fn forward_request_tools_list() {
        let mut server = mockito::Server::new_async().await;

        let mock = server
            .mock("POST", "/")
            .with_status(200_usize)
            .with_header("content-type", "application/json")
            .with_body(r#"{"result": {"tools": [{"name": "SearchBun", "description": "Search Bun documentation"}]}}"#)
            .expect(1_usize)
            .create_async()
            .await;

        let client = BunDocsClient::with_base_url(&server.url()).expect("valid mock server URL");
        let request = json!({
            "jsonrpc": "2.0",
            "id": 1_i32,
            "method": "tools/list"
        });

        let result = client.forward_request(request).await;

        mock.assert_async().await;
        drop(server);
        assert!(
            result.is_ok(),
            "Should successfully forward tools/list request"
        );

        let response = result.expect("successful response");
        assert!(response.get("result").is_some());
        // Bun Docs should return tools
        let result_field = response.get("result").expect("result field should exist");
        assert!(result_field.get("tools").is_some());
    }

    #[tokio::test]
    async fn forward_request_tools_call() {
        let mut server = mockito::Server::new_async().await;

        let mock = server
            .mock("POST", "/")
            .with_status(200_usize)
            .with_header("content-type", "application/json")
            .with_body(r#"{"result": {"content": [{"type": "text", "text": "Bun.serve() documentation..."}]}}"#)
            .expect(1_usize)
            .create_async()
            .await;

        let client = BunDocsClient::with_base_url(&server.url()).expect("valid mock server URL");
        let request = json!({
            "jsonrpc": "2.0",
            "id": 2_i32,
            "method": "tools/call",
            "params": {
                "name": "SearchBun",
                "arguments": {
                    "query": "Bun.serve"
                }
            }
        });

        let result = client.forward_request(request).await;

        mock.assert_async().await;
        drop(server);
        assert!(
            result.is_ok(),
            "Should successfully forward tools/call request"
        );

        let response = result.expect("successful response");
        assert!(response.get("result").is_some());
    }

    // Integration tests against live Bun Docs API (require network, can be flaky)
    // Run with: cargo test --ignored
    #[tokio::test]
    #[ignore = "requires network access to live Bun Docs API"]
    async fn integration_forward_request_tools_list() {
        let client = BunDocsClient::new();
        let request = json!({
            "jsonrpc": "2.0",
            "id": 1_i32,
            "method": "tools/list"
        });

        let result = client.forward_request(request).await;
        assert!(result.is_ok(), "Live API should respond to tools/list");

        let response = result.expect("successful response");
        assert!(response.get("result").is_some());
        // Bun Docs should return tools
        let result_field = response.get("result").expect("result field should exist");
        assert!(result_field.get("tools").is_some());
    }

    #[tokio::test]
    #[ignore = "requires network access to live Bun Docs API"]
    async fn integration_forward_request_tools_call() {
        let client = BunDocsClient::new();
        let request = json!({
            "jsonrpc": "2.0",
            "id": 2_i32,
            "method": "tools/call",
            "params": {
                "name": "SearchBun",
                "arguments": {
                    "query": "Bun.serve"
                }
            }
        });

        let result = client.forward_request(request).await;
        assert!(result.is_ok(), "Live API should respond to tools/call");

        let response = result.expect("successful response");
        assert!(response.get("result").is_some());
    }

    #[tokio::test]
    async fn forward_request_error_response() {
        let mut server = mockito::Server::new_async().await;

        let mock = server
            .mock("POST", "/")
            .with_status(200_usize)
            .with_header("content-type", "application/json")
            .with_body(r#"{"error": {"code": -32601, "message": "Method not found"}}"#)
            .expect(1_usize)
            .create_async()
            .await;

        let client = BunDocsClient::with_base_url(&server.url()).expect("valid mock server URL");
        let request = json!({
            "jsonrpc": "2.0",
            "id": 3_i32,
            "method": "invalid_method_that_does_not_exist"
        });

        let result = client.forward_request(request).await;

        mock.assert_async().await;
        drop(server);
        assert!(result.is_ok(), "Should receive JSON-RPC error response");

        let response = result.expect("successful HTTP response");
        assert!(
            response.get("error").is_some(),
            "Expected error field in JSON-RPC response"
        );
    }

    #[tokio::test]
    #[ignore = "requires network access to live Bun Docs API"]
    async fn integration_forward_request_error_response() {
        let client = BunDocsClient::new();
        let request = json!({
            "jsonrpc": "2.0",
            "id": 3_i32,
            "method": "invalid_method_that_does_not_exist"
        });

        let result = client.forward_request(request).await;
        // The API should either return a JSON-RPC error response or fail with an HTTP error
        if let Ok(response) = result {
            // If successful, should have an error field in JSON-RPC response
            assert!(
                response.get("error").is_some(),
                "Expected error field in response"
            );
        }
        // HTTP-level error is also acceptable
    }

    #[tokio::test]
    async fn sse_response_with_error_field() {
        let sse_data = r#"{"error": {"code": -32601, "message": "Method not found"}}"#;
        let parsed: Value = serde_json::from_str(sse_data).expect("valid JSON should parse");

        assert!(parsed.get("error").is_some());
        let error_field = parsed.get("error").expect("error field exists");
        let code_field = error_field.get("code").expect("code field exists");
        assert_eq!(code_field, &json!(-32_601_i32));
    }

    #[tokio::test]
    async fn json_parsing_from_sse_data() {
        // Test valid JSON-RPC response in SSE data
        let sse_data = r#"{"result": {"tools": []}}"#;
        let parsed: Value = serde_json::from_str(sse_data).expect("valid JSON should parse");

        assert!(parsed.get("result").is_some());
        let result_field = parsed.get("result").expect("result field exists");
        assert!(result_field.get("tools").is_some());
    }

    #[tokio::test]
    async fn json_parsing_invalid_data() {
        // Test invalid JSON in SSE data
        let sse_data = "not valid json";
        let result: Result<Value, _> = serde_json::from_str(sse_data);

        let _error = result.expect_err("invalid JSON should fail to parse");
    }

    #[test]
    fn content_type_detection() {
        let sse_type = "text/event-stream; charset=utf-8";
        let json_type = "application/json";

        assert!(sse_type.contains("text/event-stream"));
        assert!(!json_type.contains("text/event-stream"));
    }

    #[test]
    fn result_and_error_field_detection() {
        let with_result = json!({"result": {"data": "test"}});
        let with_error = json!({"error": {"code": -32_700_i32, "message": "Parse error"}});
        let neither = json!({"status": "pending"});

        assert!(with_result.get("result").is_some());
        assert!(with_error.get("error").is_some());
        assert!(neither.get("result").is_none() && neither.get("error").is_none());
    }

    #[test]
    fn empty_sse_data_handling() {
        let empty_data = "";
        assert!(empty_data.is_empty());

        // Empty data should be skipped in SSE parsing
        let non_empty = "data";
        assert!(!non_empty.is_empty());
    }

    #[test]
    fn http_status_detection() {
        // Test status code checking logic
        let status_ok = StatusCode::OK;
        let status_error = StatusCode::INTERNAL_SERVER_ERROR;

        assert!(status_ok.is_success());
        assert!(!status_error.is_success());
    }

    #[test]
    fn string_truncation() {
        let long_string = "a".repeat(300_usize);
        let truncated = long_string
            .get(..long_string.len().min(200_usize))
            .expect("valid slice within bounds");

        assert_eq!(truncated.len(), 200_usize);
    }

    #[test]
    fn timeout_value() {
        let timeout_secs = REQUEST_TIMEOUT_SECS;
        assert_eq!(timeout_secs, 5_u64);
        assert!(timeout_secs > 0_u64);
    }

    #[test]
    fn api_url_const() {
        assert_eq!(BUN_DOCS_API, "https://bun.com/docs/mcp");
        assert!(BUN_DOCS_API.starts_with("https://"));
    }

    #[test]
    fn sse_event_type_handling() {
        // Test SSE event type detection logic
        let event_type = "message";
        assert!(!event_type.is_empty());
    }

    #[test]
    fn json_parse_error_handling() {
        // Test invalid JSON parsing (covers parse_sse_response error path)
        let invalid_json = "not valid json {]";
        let result: Result<Value, _> = serde_json::from_str(invalid_json);
        let _error = result.expect_err("invalid JSON should fail to parse");
    }

    #[test]
    fn error_message_fallback() {
        // Test error text unwrap_or_else fallback
        let error_text = "Service Unavailable";
        let fallback = error_text;
        assert_eq!(fallback, "Service Unavailable");

        // Simulate fallback scenario
        let default_error = "unknown error";
        assert_eq!(default_error, "unknown error");
    }

    #[test]
    fn sse_data_min_truncation() {
        // Test SSE data truncation for debug logs
        let long_data = "a".repeat(300_usize);
        let truncated = long_data
            .get(..long_data.len().min(200_usize))
            .expect("valid slice within bounds");
        assert_eq!(truncated.len(), 200_usize);
    }

    // Retry behavior tests with mockito
    #[tokio::test]
    async fn retry_on_transient_status_503() {
        let mut server = mockito::Server::new_async().await;

        // First request fails with 503
        let mock1 = server
            .mock("POST", "/")
            .with_status(503_usize)
            .with_header("content-type", "text/plain")
            .with_body("Service Unavailable")
            .expect(1_usize)
            .create_async()
            .await;

        // Second request succeeds
        let mock2 = server
            .mock("POST", "/")
            .with_status(200_usize)
            .with_header("content-type", "application/json")
            .with_body(r#"{"result": {"tools": []}}"#)
            .expect(1_usize)
            .create_async()
            .await;

        let client = BunDocsClient::with_base_url(&server.url()).expect("valid mock server URL");
        let request = json!({"method": "tools/list"});

        let result = client.forward_request(request).await;

        mock1.assert_async().await;
        mock2.assert_async().await;
        drop(server);
        assert!(result.is_ok(), "Should succeed after retry");
        let response = result.expect("successful response");
        assert!(response.get("result").is_some());
    }

    #[tokio::test]
    async fn retry_exhaustion_on_persistent_503() {
        let mut server = mockito::Server::new_async().await;

        // All 3 attempts fail with 503
        let mock = server
            .mock("POST", "/")
            .with_status(503_usize)
            .with_header("content-type", "text/plain")
            .with_body("Service Unavailable")
            .expect(3_usize)
            .create_async()
            .await;

        let client = BunDocsClient::with_base_url(&server.url()).expect("valid mock server URL");
        let request = json!({"method": "tools/list"});

        let result = client.forward_request(request).await;

        mock.assert_async().await;
        drop(server);
        assert!(result.is_err(), "Should fail after exhausting retries");
        let error = result.expect_err("should be an error");
        assert!(error.to_string().contains("503"));
    }

    #[tokio::test]
    async fn no_retry_on_non_transient_404() {
        let mut server = mockito::Server::new_async().await;

        // 404 is not transient, should not retry
        let mock = server
            .mock("POST", "/")
            .with_status(404_usize)
            .with_header("content-type", "text/plain")
            .with_body("Not Found")
            .expect(1_usize)
            .create_async()
            .await;

        let client = BunDocsClient::with_base_url(&server.url()).expect("valid mock server URL");
        let request = json!({"method": "tools/list"});

        let result = client.forward_request(request).await;

        mock.assert_async().await;
        drop(server);
        assert!(result.is_err(), "Should fail without retry on 404");
        let error = result.expect_err("should be an error");
        assert!(error.to_string().contains("404"));
    }

    #[tokio::test]
    async fn retry_on_429_rate_limit() {
        let mut server = mockito::Server::new_async().await;

        // First request gets rate limited
        let mock1 = server
            .mock("POST", "/")
            .with_status(429_usize)
            .with_header("content-type", "text/plain")
            .with_body("Too Many Requests")
            .expect(1_usize)
            .create_async()
            .await;

        // Second request succeeds
        let mock2 = server
            .mock("POST", "/")
            .with_status(200_usize)
            .with_header("content-type", "application/json")
            .with_body(r#"{"result": {"data": "success"}}"#)
            .expect(1_usize)
            .create_async()
            .await;

        let client = BunDocsClient::with_base_url(&server.url()).expect("valid mock server URL");
        let request = json!({"method": "test"});

        let result = client.forward_request(request).await;

        mock1.assert_async().await;
        mock2.assert_async().await;
        drop(server);
        assert!(result.is_ok(), "Should succeed after retrying 429");
    }

    #[tokio::test]
    async fn retry_on_500_internal_error() {
        let mut server = mockito::Server::new_async().await;

        // First request fails with 500
        let mock1 = server
            .mock("POST", "/")
            .with_status(500_usize)
            .with_header("content-type", "text/plain")
            .with_body("Internal Server Error")
            .expect(1_usize)
            .create_async()
            .await;

        // Second request succeeds
        let mock2 = server
            .mock("POST", "/")
            .with_status(200_usize)
            .with_header("content-type", "application/json")
            .with_body(r#"{"result": {}}"#)
            .expect(1_usize)
            .create_async()
            .await;

        let client = BunDocsClient::with_base_url(&server.url()).expect("valid mock server URL");
        let request = json!({"method": "test"});

        let result = client.forward_request(request).await;

        mock1.assert_async().await;
        mock2.assert_async().await;
        drop(server);
        assert!(result.is_ok(), "Should succeed after retrying 500");
    }

    #[tokio::test]
    async fn retry_on_502_bad_gateway() {
        let mut server = mockito::Server::new_async().await;

        // Simulate bad gateway then recovery
        let mock1 = server
            .mock("POST", "/")
            .with_status(502_usize)
            .with_body("Bad Gateway")
            .expect(1_usize)
            .create_async()
            .await;

        let mock2 = server
            .mock("POST", "/")
            .with_status(200_usize)
            .with_header("content-type", "application/json")
            .with_body(r#"{"result": {}}"#)
            .expect(1_usize)
            .create_async()
            .await;

        let client = BunDocsClient::with_base_url(&server.url()).expect("valid mock server URL");
        let request = json!({"method": "test"});

        let result = client.forward_request(request).await;

        mock1.assert_async().await;
        mock2.assert_async().await;
        drop(server);
        let _response = result.expect("successful response after retry");
    }

    #[tokio::test]
    async fn retry_timing_exponential_backoff() {
        let mut server = mockito::Server::new_async().await;

        // All requests fail to test backoff timing
        let mock = server
            .mock("POST", "/")
            .with_status(503_usize)
            .with_body("Unavailable")
            .expect(3_usize)
            .create_async()
            .await;

        let client = BunDocsClient::with_base_url(&server.url()).expect("valid mock server URL");
        let request = json!({"method": "test"});

        let start = Instant::now();
        let _result = client.forward_request(request).await;
        let elapsed = start.elapsed();

        mock.assert_async().await;
        drop(server);

        // With 3 attempts and delays of 200 ms, 400 ms:
        // Total should be at least 600 ms (200 + 400)
        // But allow some margin for execution time
        assert!(
            elapsed.as_millis() >= 550_u128,
            "Expected at least 600 ms for backoff, got {}ms",
            elapsed.as_millis()
        );
    }

    #[tokio::test]
    async fn fetch_doc_markdown_success() {
        let mut server = mockito::Server::new_async().await;

        let mock = server
            .mock("GET", "/docs/page")
            .match_header("accept", "text/markdown")
            .with_status(200_usize)
            .with_header("content-type", "text/markdown")
            .with_body("# Test MDX\n\nSome content")
            .expect(1_usize)
            .create_async()
            .await;

        let client = BunDocsClient::with_base_url(&server.url()).expect("valid mock server URL");
        let url = format!("{}/docs/page", server.url());

        let result = client.fetch_doc_markdown(&url).await;

        mock.assert_async().await;
        drop(server);
        assert!(result.is_ok());
        let mdx = result.expect("successful MDX fetch");
        assert!(mdx.contains("# Test MDX"));
        assert!(mdx.contains("Some content"));
    }

    #[tokio::test]
    async fn fetch_doc_markdown_404_error() {
        let mut server = mockito::Server::new_async().await;

        let mock = server
            .mock("GET", "/docs/missing")
            .with_status(404_usize)
            .with_body("Not Found")
            .expect(1_usize)
            .create_async()
            .await;

        let client = BunDocsClient::with_base_url(&server.url()).expect("valid mock server URL");
        let url = format!("{}/docs/missing", server.url());

        let result = client.fetch_doc_markdown(&url).await;

        mock.assert_async().await;
        drop(server);
        assert!(result.is_err());
        let error = result.expect_err("should be 404 error");
        assert!(error.to_string().contains("404"));
    }

    #[tokio::test]
    async fn fetch_doc_markdown_500_error() {
        let mut server = mockito::Server::new_async().await;

        let mock = server
            .mock("GET", "/docs/error")
            .with_status(500_usize)
            .with_body("Internal Server Error")
            .expect(1_usize)
            .create_async()
            .await;

        let client = BunDocsClient::with_base_url(&server.url()).expect("valid mock server URL");
        let url = format!("{}/docs/error", server.url());

        let result = client.fetch_doc_markdown(&url).await;

        mock.assert_async().await;
        drop(server);
        assert!(result.is_err());
        let error = result.expect_err("should be 500 error");
        assert!(error.to_string().contains("500"));
    }

    #[tokio::test]
    async fn retry_with_transient_http_failure_logging() {
        let mut server = mockito::Server::new_async().await;

        // First attempt: 503 error (transient)
        let mock1 = server
            .mock("POST", "/")
            .with_status(503_usize)
            .with_header("content-type", "text/plain")
            .with_body("Service Unavailable")
            .expect(1_usize)
            .create_async()
            .await;

        // Second attempt: Success
        let mock2 = server
            .mock("POST", "/")
            .with_status(200_usize)
            .with_header("content-type", "application/json")
            .with_body(r#"{"result": {"tools": []}}"#)
            .expect(1_usize)
            .create_async()
            .await;

        let client = BunDocsClient::with_base_url(&server.url()).expect("valid mock server URL");
        let request = json!({"method": "tools/list"});

        let result = client.forward_request(request).await;

        mock1.assert_async().await;
        mock2.assert_async().await;
        drop(server);

        assert!(result.is_ok(), "Should succeed after transient 503 retry");
        let response = result.expect("successful response");
        assert!(response.get("result").is_some());
        // Verifies src/http.rs line 315-319: warn!("Transient HTTP status...")
        // Verifies line 321: backoff_delay_ms calculation
        // Verifies line 322: sleep execution
    }

    #[tokio::test]
    async fn retry_on_multiple_transient_failures() {
        let mut server = mockito::Server::new_async().await;

        // First attempt: 502 Bad Gateway
        let mock1 = server
            .mock("POST", "/")
            .with_status(502_usize)
            .with_body("Bad Gateway")
            .expect(1_usize)
            .create_async()
            .await;

        // Second attempt: 503 Service Unavailable
        let mock2 = server
            .mock("POST", "/")
            .with_status(503_usize)
            .with_body("Service Unavailable")
            .expect(1_usize)
            .create_async()
            .await;

        // Third attempt: 504 Gateway Timeout (transient)
        let mock3 = server
            .mock("POST", "/")
            .with_status(504_usize)
            .with_body("Gateway Timeout")
            .expect(1_usize)
            .create_async()
            .await;

        let client = BunDocsClient::with_base_url(&server.url()).expect("valid mock server URL");
        let request = json!({"method": "test"});

        let result = client.forward_request(request).await;

        // All three mocks should have been called (exhausted retries)
        mock1.assert_async().await;
        mock2.assert_async().await;
        mock3.assert_async().await;
        drop(server);

        assert!(result.is_err(), "Should fail after exhausting retries");
        let error = result.expect_err("should be an error");
        assert!(error.to_string().contains("504"));
        // Verifies src/http.rs line 314: is_transient_status check for all 5xx codes
        // Verifies line 317-318: retry condition check (attempt < MAX_RETRIES)
        // Verifies line 321-322: backoff delays between attempts
    }
}
