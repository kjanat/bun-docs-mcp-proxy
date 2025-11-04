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
use core::time::Duration;
use eventsource_stream::Eventsource as _;
use futures::StreamExt as _;
use reqwest::{Client, StatusCode, Url, header::HeaderMap};
use serde_json::Value;
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
        // Safe: attempt.saturating_sub(1) will be small in practice (<= MAX_RETRIES=3)
        #[expect(clippy::cast_possible_truncation)]
        let base = BACKOFF_BASE_MS.saturating_mul(1u64 << (attempt.saturating_sub(1) as u32));
        base.min(BACKOFF_MAX_MS)
    }

    /// Check if HTTP status code indicates a transient error worth retrying
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

                    if status.is_success() {
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
                    } else {
                        // Read body (truncated) for context
                        // Limit to 100KB to prevent OOM from malicious/misconfigured servers
                        const MAX_ERROR_BODY_SIZE: usize = 100_000;
                        let bytes = response.bytes().await.unwrap_or_else(|e| {
                            warn!("Failed to read error response body: {}", e);
                            Bytes::default()
                        });
                        let limited_bytes = if bytes.len() > MAX_ERROR_BODY_SIZE {
                            &bytes[..MAX_ERROR_BODY_SIZE]
                        } else {
                            &*bytes
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
                    }
                }
                Err(e) => {
                    // Connection/timeout/etc. Retry if transient
                    let is_transient = e.is_connect() || e.is_timeout() || e.is_request();
                    let err = anyhow::anyhow!("Failed to send request to Bun Docs API: {e}");

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
mod tests;
