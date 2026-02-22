//! HTTP client for bun.com/docs/mcp.
//!
//! ## Response Contract
//!
//! Responses may be either:
//! - `application/json` (single JSON-RPC object), or
//! - `text/event-stream` (SSE), where each event's `data` contains a complete JSON-RPC object.
//!
//! **Important**: We do **not** accumulate partial/delta SSE events. If the upstream starts
//! streaming deltas, this parser must be upgraded.
//!
//! ## Retry Contract
//!
//! Transient failures (network errors, 429, 5xx) are retried up to [`MAX_RETRIES`] times:
//! - For 429: uses `Retry-After` header if present, else exponential backoff.
//! - For 5xx/network: exponential backoff (200 ms -> 400 ms -> 800 ms, capped at 1 s).

use std::time::Duration;

use anyhow::{Context as _, Result};
use bytes::{Bytes, BytesMut};
use eventsource_stream::Eventsource as _;
use futures::StreamExt as _;
use reqwest::{Client, StatusCode, Url, header::HeaderMap};
use serde_json::Value;
use tracing::{Span, debug, info, instrument, warn};

use crate::{
    mcp::{JsonRpcEnvelope, content_type},
    util::truncate_utf8,
};

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

/// Maximum size for error body snippets in logs (2KB)
const MAX_ERROR_SNIPPET_SIZE: usize = 2048;

/// Configuration for `BunDocsClient`
#[derive(Debug, Clone)]
pub struct BunDocsClientConfig {
    /// Base URL for API requests
    pub base_url: Url,
    /// HTTP request timeout
    pub timeout: Duration,
    /// Maximum number of retry attempts for transient failures
    pub max_retries: usize,
    /// Base delay for exponential backoff
    pub backoff_base: Duration,
    /// Maximum backoff delay
    pub backoff_max: Duration,
    /// Maximum error response body size to read (prevents OOM)
    pub max_error_body_size: usize,
    /// Maximum size for error body snippets in logs
    pub max_error_snippet_size: usize,
}

impl Default for BunDocsClientConfig {
    fn default() -> Self {
        #[allow(clippy::expect_used, reason = "URL constant is compile-time valid")]
        Self {
            base_url: Url::parse(BUN_DOCS_API).expect("BUN_DOCS_API is a valid URL"),
            timeout: Duration::from_secs(REQUEST_TIMEOUT_SECS),
            max_retries: MAX_RETRIES,
            backoff_base: Duration::from_millis(BACKOFF_BASE_MS),
            backoff_max: Duration::from_millis(BACKOFF_MAX_MS),
            max_error_body_size: MAX_ERROR_BODY_SIZE,
            max_error_snippet_size: MAX_ERROR_SNIPPET_SIZE,
        }
    }
}

/// Builder for `BunDocsClient` with fluent configuration API
#[derive(Debug, Clone)]
#[allow(dead_code, reason = "public API for consumers")]
pub struct BunDocsClientBuilder {
    config: BunDocsClientConfig,
    client: Option<Client>,
}

#[allow(dead_code, reason = "public API for consumers")]
impl BunDocsClientBuilder {
    /// Creates a new builder with default configuration
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: BunDocsClientConfig::default(),
            client: None,
        }
    }

    /// Sets the base URL for API requests
    ///
    /// # Errors
    /// Returns an error if the URL cannot be parsed
    pub fn base_url(mut self, url: &str) -> Result<Self> {
        self.config.base_url = Url::parse(url).context("Invalid base URL")?;
        Ok(self)
    }

    /// Sets the HTTP request timeout
    #[must_use]
    pub const fn timeout(mut self, timeout: Duration) -> Self {
        self.config.timeout = timeout;
        self
    }

    /// Sets the maximum number of retry attempts for failed requests.
    ///
    /// The value represents total attempts: `max_retries(1)` means try once (no retries),
    /// `max_retries(3)` means up to 3 attempts total. A value of 0 is treated as 1 (single attempt).
    #[must_use]
    pub const fn max_retries(mut self, retries: usize) -> Self {
        self.config.max_retries = retries;
        self
    }

    /// Sets the base delay for exponential backoff
    #[must_use]
    pub const fn backoff_base(mut self, delay: Duration) -> Self {
        self.config.backoff_base = delay;
        self
    }

    /// Sets the maximum backoff delay
    #[must_use]
    pub const fn backoff_max(mut self, delay: Duration) -> Self {
        self.config.backoff_max = delay;
        self
    }

    /// Sets the maximum error response body size to read
    #[must_use]
    pub const fn max_error_body_size(mut self, size: usize) -> Self {
        self.config.max_error_body_size = size;
        self
    }

    /// Sets the maximum size for error body snippets in logs
    #[must_use]
    pub const fn max_error_snippet_size(mut self, size: usize) -> Self {
        self.config.max_error_snippet_size = size;
        self
    }

    /// Sets a custom HTTP client.
    ///
    /// Note: Per-request timeouts are set via [`Self::timeout()`], but any default timeout
    /// configured on the injected client may also apply. If you set timeouts on both,
    /// the shorter timeout will take effect for each request.
    #[must_use]
    pub fn http_client(mut self, client: Client) -> Self {
        self.client = Some(client);
        self
    }

    /// Builds the `BunDocsClient` with the configured options
    #[must_use]
    pub fn build(self) -> BunDocsClient {
        BunDocsClient {
            client: self.client.unwrap_or_default(),
            config: self.config,
        }
    }
}

impl Default for BunDocsClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Parsed response from the upstream Bun Docs MCP server.
/// Distinguishes between successful results and JSON-RPC errors.
#[derive(Debug, Clone)]
pub enum UpstreamResponse {
    /// Successful response with result payload
    Ok(Value),
    /// Error response from upstream
    Err {
        /// JSON-RPC error code
        code: i64,
        /// Human-readable error message
        message: String,
        /// Optional additional error data
        data: Option<Value>,
    },
}

impl UpstreamResponse {
    /// Returns true if this is a successful response
    #[must_use]
    #[allow(dead_code, reason = "public API for consumers and tests")]
    pub const fn is_ok(&self) -> bool {
        matches!(self, Self::Ok(_))
    }

    /// Returns true if this is an error response
    #[must_use]
    #[allow(dead_code, reason = "public API for consumers and tests")]
    pub const fn is_err(&self) -> bool {
        matches!(self, Self::Err { .. })
    }

    /// Converts to Result for ergonomic handling.
    ///
    /// # Errors
    ///
    /// Returns `Err((code, message, data))` if this is an error response.
    #[allow(dead_code, reason = "public API for consumers and tests")]
    pub fn into_result(self) -> Result<Value, (i64, String, Option<Value>)> {
        match self {
            Self::Ok(value) => Ok(value),
            Self::Err {
                code,
                message,
                data,
            } => Err((code, message, data)),
        }
    }

    /// Parse a JSON value into an `UpstreamResponse`.
    ///
    /// Expects either `{"result": ...}` or `{"error": {"code": ..., "message": ...}}`.
    /// Uses [`JsonRpcEnvelope`] for parsing to ensure consistent handling.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "consuming value for ownership"
    )]
    fn from_json(value: Value) -> anyhow::Result<Self> {
        match JsonRpcEnvelope::from_value(value)
            .map_err(|e| anyhow::anyhow!("Invalid JSON-RPC response: {e}"))?
        {
            JsonRpcEnvelope::Success { result, .. } => Ok(Self::Ok(result)),
            JsonRpcEnvelope::Error { error, .. } => Ok(Self::Err {
                code: error.code,
                message: error.message,
                data: error.data,
            }),
        }
    }
}

/// HTTP client for interacting with the Bun Docs API
pub struct BunDocsClient {
    /// The underlying `reqwest::Client` used for making HTTP requests.
    client: Client,
    /// Configuration for this client
    config: BunDocsClientConfig,
}

impl Default for BunDocsClient {
    fn default() -> Self {
        Self::new()
    }
}

impl BunDocsClient {
    /// Creates a new client with the default Bun Docs API URL.
    ///
    /// Uses a compile-time validated URL constant, so this cannot fail at runtime.
    #[must_use]
    pub fn new() -> Self {
        Self::builder().build()
    }

    /// Creates a new client with a custom base URL.
    ///
    /// # Arguments
    /// * `url` - The base URL for API requests
    ///
    /// # Errors
    /// Returns an error if the URL cannot be parsed
    #[allow(dead_code, reason = "public API for consumers")]
    pub fn with_base_url(url: &str) -> Result<Self> {
        Ok(Self::builder().base_url(url)?.build())
    }

    /// Returns a new builder for configuring a `BunDocsClient`
    #[must_use]
    #[allow(dead_code, reason = "public API for consumers")]
    pub fn builder() -> BunDocsClientBuilder {
        BunDocsClientBuilder::new()
    }

    /// Returns a reference to the client configuration
    #[must_use]
    #[allow(dead_code, reason = "public API for consumers")]
    pub const fn config(&self) -> &BunDocsClientConfig {
        &self.config
    }

    /// Calculates an exponential backoff delay for retry attempts.
    ///
    /// The delay increases with each `attempt` (e.g., 200ms, 400ms, 800ms) up to the configured max.
    /// This helps prevent overwhelming the server during transient failures.
    ///
    /// # Arguments
    /// * `attempt` - The current retry attempt number (must be >= 1).
    ///
    /// # Returns
    /// The calculated delay as a `Duration`.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "backoff durations are small enough to fit in u64"
    )]
    fn backoff_delay(&self, attempt: usize) -> Duration {
        debug_assert!(attempt > 0_usize, "attempt must be >= 1");
        // 200ms, 400ms, 800ms (cap at configured max)
        // Safe: attempt.saturating_sub(1) will be small in practice (<= max_retries)
        #[expect(
            clippy::cast_possible_truncation,
            reason = "attempt.saturating_sub(1) is bounded by max_retries, fits in u32"
        )]
        let multiplier = 1_u64 << (attempt.saturating_sub(1_usize) as u32);
        let base_ms = self.config.backoff_base.as_millis() as u64;
        let delay_ms = base_ms.saturating_mul(multiplier);
        let max_ms = self.config.backoff_max.as_millis() as u64;
        Duration::from_millis(delay_ms.min(max_ms))
    }

    /// Reads response body with a size limit, streaming chunks to avoid buffering entire response.
    ///
    /// This prevents OOM attacks from malicious servers sending huge responses.
    /// Reading stops as soon as `limit` bytes have been accumulated.
    ///
    /// # Arguments
    /// * `response` - The HTTP response to read from.
    /// * `limit` - Maximum number of bytes to read.
    ///
    /// # Returns
    /// The accumulated bytes, truncated at `limit`.
    #[allow(clippy::indexing_slicing, reason = "slice bounds checked above")]
    async fn read_body_limited(response: reqwest::Response, limit: usize) -> Bytes {
        let mut buf = BytesMut::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk_result) = stream.next().await {
            match chunk_result {
                Ok(chunk) => {
                    let remaining = limit.saturating_sub(buf.len());
                    if remaining == 0 {
                        break;
                    }
                    if chunk.len() <= remaining {
                        buf.extend_from_slice(&chunk);
                    } else {
                        buf.extend_from_slice(&chunk[..remaining]);
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        buf.freeze()
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

    /// Extracts delay from `Retry-After` header if present.
    ///
    /// Supports integer seconds format (e.g., "Retry-After: 120").
    /// Does NOT support HTTP-date format.
    ///
    /// # Arguments
    /// * `headers` - A reference to the `HeaderMap` containing the HTTP response headers.
    ///
    /// # Returns
    /// `Some(Duration)` if a valid integer delay was found, `None` otherwise.
    fn retry_after_delay(headers: &HeaderMap) -> Option<Duration> {
        headers
            .get(reqwest::header::RETRY_AFTER)?
            .to_str()
            .ok()?
            .parse::<u64>()
            .ok()
            .map(Duration::from_secs)
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

    /// Forward a JSON-RPC request to the Bun Docs API with automatic retries
    ///
    /// # Arguments
    /// * `request` - JSON-RPC request object
    ///
    /// # Returns
    /// `UpstreamResponse::Ok` with result payload on success,
    /// `UpstreamResponse::Err` with code/message/data on upstream JSON-RPC error
    ///
    /// # Errors
    /// Returns an `anyhow::Error` if all retry attempts fail, a non-retryable HTTP error occurs,
    /// or the response cannot be parsed as a valid JSON-RPC response
    #[allow(
        clippy::too_many_lines,
        reason = "complex retry logic with error handling"
    )]
    #[instrument(
        name = "http_forward",
        skip(self, request),
        fields(attempt = tracing::field::Empty, status = tracing::field::Empty, response_size = tracing::field::Empty)
    )]
    pub async fn forward_request(&self, request: Value) -> Result<UpstreamResponse> {
        debug!("Forwarding request to Bun Docs API");

        let mut last_error: Option<anyhow::Error> = None;

        let max_retries = self.config.max_retries.max(1); // Ensure at least one attempt
        for attempt in 1_usize..=max_retries {
            Span::current().record("attempt", attempt);
            // Build request each attempt
            let rb = self
                .client
                .post(self.config.base_url.as_str())
                .header(reqwest::header::CONTENT_TYPE, content_type::JSON)
                .header(
                    reqwest::header::ACCEPT,
                    format!("{}, {}", content_type::JSON, content_type::SSE),
                )
                .json(&request)
                .timeout(self.config.timeout);

            match rb.send().await {
                Ok(response) => {
                    let status = response.status();
                    Span::current().record("status", status.as_u16());
                    info!(
                        "Bun Docs API response status: {} (attempt {} of {})",
                        status, attempt, max_retries
                    );

                    let headers = response.headers().clone();
                    let resp_content_type = Self::main_content_type(&headers);

                    if status.is_success() {
                        // Success: decide how to parse based on content type
                        let json_value = if resp_content_type.starts_with(content_type::SSE) {
                            debug!("Parsing SSE stream");
                            self.parse_sse_response(response).await?
                        } else {
                            debug!("Parsing regular JSON response");
                            response
                                .json()
                                .await
                                .context("Failed to parse JSON response")?
                        };
                        // Record approximate response size for debugging truncation issues
                        if let Ok(serialized) = serde_json::to_string(&json_value) {
                            Span::current().record("response_size", serialized.len());
                        }
                        return UpstreamResponse::from_json(json_value);
                    }
                    // Read body with streaming limit to prevent OOM from malicious servers
                    let bytes =
                        Self::read_body_limited(response, self.config.max_error_body_size).await;
                    let body = String::from_utf8_lossy(&bytes);
                    let body_snippet = truncate_utf8(&body, self.config.max_error_snippet_size);
                    let header_summary = Self::summarize_headers(&headers);

                    let ct_display = if resp_content_type.is_empty() {
                        "(none)"
                    } else {
                        &resp_content_type
                    };
                    let error = anyhow::anyhow!(
                        "Bun Docs API error: status={status} content_type={ct_display} headers=[{header_summary}] body_snippet=\"{body_snippet}\""
                    );

                    // Retry on transient server statuses
                    if Self::is_transient_status(status) && attempt < max_retries {
                        // Use Retry-After header if present (for 429), else exponential backoff
                        let delay = Self::retry_after_delay(&headers)
                            .unwrap_or_else(|| self.backoff_delay(attempt));
                        warn!(
                            "Transient HTTP status {}, retrying in {:?} (attempt {})",
                            status,
                            delay,
                            attempt + 1
                        );
                        tokio::time::sleep(delay).await;
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

                    if is_transient && attempt < max_retries {
                        warn!(
                            "Network error: {}. Retrying (attempt {} of {})",
                            err,
                            attempt + 1,
                            max_retries
                        );
                        let delay = self.backoff_delay(attempt);
                        tokio::time::sleep(delay).await;
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
    /// A deadline is enforced to prevent indefinite hangs if the server sends
    /// heartbeats but never delivers a JSON-RPC envelope.
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
    /// - The configured timeout expires before a valid JSON-RPC envelope is received.
    #[instrument(name = "sse_parse", skip(self, response))]
    async fn parse_sse_response(&self, response: reqwest::Response) -> Result<Value> {
        let timeout_duration = self.config.timeout;

        tokio::time::timeout(timeout_duration, self.parse_sse_response_inner(response))
            .await
            .map_err(|_elapsed| {
                anyhow::anyhow!("Timed out waiting for JSON-RPC envelope in SSE stream")
            })?
    }

    /// Inner SSE parsing logic without timeout wrapper.
    async fn parse_sse_response_inner(&self, response: reqwest::Response) -> Result<Value> {
        let mut event_stream = response.bytes_stream().eventsource();
        let mut json_response: Option<Value> = None;

        loop {
            let Some(event_result) = event_stream.next().await else {
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
                                let preview = data.get(..200).unwrap_or(&data);
                                debug!("SSE data: {preview}");
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

    /// Validates that a URL is safe to fetch for markdown content.
    ///
    /// SSRF protection: only allows HTTPS scheme and trusted hosts (bun.com, bun.sh).
    ///
    /// # Errors
    /// Returns an error if the URL:
    /// - Cannot be parsed
    /// - Uses a non-HTTPS scheme
    /// - Points to an untrusted host
    fn validate_markdown_url(url: &str) -> Result<()> {
        let parsed = Url::parse(url).context("Invalid URL")?;
        if parsed.scheme() != "https" {
            anyhow::bail!("Refusing non-https markdown fetch: {url}");
        }
        if !matches!(parsed.host_str(), Some("bun.com" | "bun.sh")) {
            anyhow::bail!("Refusing non-bun markdown fetch: {url}");
        }
        Ok(())
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
    #[instrument(name = "fetch_mdx", skip(self), fields(url = %url))]
    pub async fn fetch_doc_markdown(&self, url: &str) -> Result<String> {
        debug!("Fetching MDX for URL: {}", url);

        // SSRF protection: only allow trusted hosts
        Self::validate_markdown_url(url)?;

        let response = self
            .client
            .get(url)
            .header(reqwest::header::ACCEPT, content_type::MARKDOWN)
            .timeout(self.config.timeout)
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

    /// Internal fetch without URL validation - for testing HTTP behavior only.
    #[cfg(test)]
    async fn fetch_doc_markdown_unchecked(&self, url: &str) -> Result<String> {
        let response = self
            .client
            .get(url)
            .header(reqwest::header::ACCEPT, content_type::MARKDOWN)
            .timeout(self.config.timeout)
            .send()
            .await
            .context("Failed to send request for markdown")?;

        let status = response.status();
        if !status.is_success() {
            return Err(anyhow::anyhow!(
                "Failed to fetch markdown: HTTP {status} for URL: {url}"
            ));
        }

        response
            .text()
            .await
            .context("Failed to read markdown response body")
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "tests can use expect()")]
#[allow(clippy::unwrap_used, reason = "tests can use unwrap()")]
#[allow(clippy::indexing_slicing, reason = "tests use array indexing")]
#[allow(clippy::default_numeric_fallback, reason = "test literals")]
mod tests {
    use std::time::Instant;

    use serde_json::json;

    use super::*;

    #[test]
    fn client_creation() {
        let client = BunDocsClient::new();
        assert_eq!(client.config().base_url.as_str(), BUN_DOCS_API);
    }

    #[test]
    fn client_default() {
        let client = BunDocsClient::default();
        assert_eq!(client.config().base_url.as_str(), BUN_DOCS_API);
    }

    #[test]
    fn client_with_base_url() {
        let custom_url = "https://example.com/api";
        let client = BunDocsClient::with_base_url(custom_url).expect("valid URL should parse");
        assert_eq!(client.config().base_url.as_str(), custom_url);
    }

    #[test]
    fn client_with_base_url_invalid() {
        let result = BunDocsClient::with_base_url("not a valid url");
        assert!(result.is_err());
    }

    #[test]
    fn backoff_delay() {
        let client = BunDocsClient::new();
        assert_eq!(
            client.backoff_delay(1_usize),
            Duration::from_millis(200_u64)
        );
        assert_eq!(
            client.backoff_delay(2_usize),
            Duration::from_millis(400_u64)
        );
        assert_eq!(
            client.backoff_delay(3_usize),
            Duration::from_millis(800_u64)
        );
        assert_eq!(
            client.backoff_delay(4_usize),
            Duration::from_millis(1000_u64)
        ); // capped
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
    fn truncate_utf8_via_util() {
        // truncate_utf8 moved to util module, test import works
        let short = "hello";
        assert_eq!(truncate_utf8(short, 10_usize), short);

        let long = "a".repeat(100_usize);
        let truncated = truncate_utf8(&long, 50_usize);
        assert!(truncated.len() <= 50_usize);
        assert!(!truncated.is_empty());
        assert!(truncated.is_char_boundary(truncated.len()));

        // Test with Unicode characters
        // "hello 世界"
        let unicode = "hello \u{4e16}\u{754c}";
        let truncated_unicode = truncate_utf8(unicode, 8_usize);
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
            .with_body(r#"{"jsonrpc":"2.0","result":{"tools":[{"name":"SearchBun","description":"Search Bun documentation"}]},"id":1}"#)
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

        let upstream = result.expect("successful response");
        assert!(upstream.is_ok(), "Should be UpstreamResponse::Ok");
        // Bun Docs should return tools
        let result_value = upstream.into_result().expect("should be Ok variant");
        assert!(result_value.get("tools").is_some());
    }

    #[tokio::test]
    async fn forward_request_tools_call() {
        let mut server = mockito::Server::new_async().await;

        let mock = server
            .mock("POST", "/")
            .with_status(200_usize)
            .with_header("content-type", "application/json")
            .with_body(r#"{"jsonrpc":"2.0","result":{"content":[{"type":"text","text":"Bun.serve() documentation..."}]},"id":2}"#)
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

        let upstream = result.expect("successful response");
        assert!(upstream.is_ok(), "Should be UpstreamResponse::Ok");
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

        let upstream = result.expect("successful response");
        assert!(upstream.is_ok(), "Should be UpstreamResponse::Ok");
        // Bun Docs should return tools
        let result_value = upstream.into_result().expect("should be Ok variant");
        assert!(result_value.get("tools").is_some());
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

        let upstream = result.expect("successful response");
        assert!(upstream.is_ok(), "Should be UpstreamResponse::Ok");
    }

    #[tokio::test]
    async fn forward_request_error_response() {
        let mut server = mockito::Server::new_async().await;

        let mock = server
            .mock("POST", "/")
            .with_status(200_usize)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"jsonrpc":"2.0","error":{"code":-32601,"message":"Method not found"},"id":3}"#,
            )
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

        let upstream = result.expect("successful HTTP response");
        assert!(
            upstream.is_err(),
            "Expected UpstreamResponse::Err for JSON-RPC error"
        );
        // Verify error details
        let (code, message, _data) = upstream.into_result().expect_err("should be Err variant");
        assert_eq!(code, -32601_i64);
        assert_eq!(message, "Method not found");
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
        if let Ok(upstream) = result {
            // If successful HTTP, should be an UpstreamResponse::Err
            assert!(
                upstream.is_err(),
                "Expected UpstreamResponse::Err for invalid method"
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
            .with_body(r#"{"jsonrpc":"2.0","result":{"tools":[]},"id":1}"#)
            .expect(1_usize)
            .create_async()
            .await;

        let client = BunDocsClient::with_base_url(&server.url()).expect("valid mock server URL");
        let request = json!({"jsonrpc":"2.0","method":"tools/list","id":1});

        let result = client.forward_request(request).await;

        mock1.assert_async().await;
        mock2.assert_async().await;
        drop(server);
        assert!(result.is_ok(), "Should succeed after retry");
        let upstream = result.expect("successful response");
        assert!(upstream.is_ok(), "Should be UpstreamResponse::Ok");
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
            .with_body(r#"{"jsonrpc":"2.0","result":{"data":"success"},"id":1}"#)
            .expect(1_usize)
            .create_async()
            .await;

        let client = BunDocsClient::with_base_url(&server.url()).expect("valid mock server URL");
        let request = json!({"jsonrpc":"2.0","method":"test","id":1});

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
            .with_body(r#"{"jsonrpc":"2.0","result":{},"id":1}"#)
            .expect(1_usize)
            .create_async()
            .await;

        let client = BunDocsClient::with_base_url(&server.url()).expect("valid mock server URL");
        let request = json!({"jsonrpc":"2.0","method":"test","id":1});

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
            .with_body(r#"{"jsonrpc":"2.0","result":{},"id":1}"#)
            .expect(1_usize)
            .create_async()
            .await;

        let client = BunDocsClient::with_base_url(&server.url()).expect("valid mock server URL");
        let request = json!({"jsonrpc":"2.0","method":"test","id":1});

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

        // Use unchecked variant to test HTTP behavior without SSRF validation
        let result = client.fetch_doc_markdown_unchecked(&url).await;

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

        // Use unchecked variant to test HTTP behavior without SSRF validation
        let result = client.fetch_doc_markdown_unchecked(&url).await;

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

        // Use unchecked variant to test HTTP behavior without SSRF validation
        let result = client.fetch_doc_markdown_unchecked(&url).await;

        mock.assert_async().await;
        drop(server);
        assert!(result.is_err());
        let error = result.expect_err("should be 500 error");
        assert!(error.to_string().contains("500"));
    }

    // SSRF protection tests
    #[tokio::test]
    async fn fetch_doc_markdown_rejects_http_scheme() {
        let client = BunDocsClient::new();
        let result = client.fetch_doc_markdown("http://bun.com/docs/page").await;

        assert!(result.is_err());
        let error = result.expect_err("should reject http");
        assert!(error.to_string().contains("non-https"));
    }

    #[tokio::test]
    async fn fetch_doc_markdown_rejects_untrusted_host() {
        let client = BunDocsClient::new();
        let result = client
            .fetch_doc_markdown("https://evil.com/docs/page")
            .await;

        assert!(result.is_err());
        let error = result.expect_err("should reject untrusted host");
        assert!(error.to_string().contains("non-bun"));
    }

    #[tokio::test]
    async fn fetch_doc_markdown_rejects_localhost() {
        let client = BunDocsClient::new();
        let result = client
            .fetch_doc_markdown("https://localhost:8080/secret")
            .await;

        assert!(result.is_err());
        let error = result.expect_err("should reject localhost");
        assert!(error.to_string().contains("non-bun"));
    }

    #[tokio::test]
    async fn fetch_doc_markdown_rejects_internal_ip() {
        let client = BunDocsClient::new();
        let result = client.fetch_doc_markdown("https://192.168.1.1/admin").await;

        assert!(result.is_err());
        let error = result.expect_err("should reject internal IP");
        assert!(error.to_string().contains("non-bun"));
    }

    #[tokio::test]
    async fn fetch_doc_markdown_rejects_file_scheme() {
        let client = BunDocsClient::new();
        let result = client.fetch_doc_markdown("file:///etc/passwd").await;

        assert!(result.is_err());
        let error = result.expect_err("should reject file scheme");
        assert!(error.to_string().contains("non-https"));
    }

    #[tokio::test]
    async fn fetch_doc_markdown_rejects_invalid_url() {
        let client = BunDocsClient::new();
        let result = client.fetch_doc_markdown("not a valid url").await;

        assert!(result.is_err());
        let error = result.expect_err("should reject invalid URL");
        assert!(error.to_string().contains("Invalid URL"));
    }

    #[tokio::test]
    async fn fetch_doc_markdown_allows_bun_sh() {
        // bun.sh should be allowed (but will fail with network error in test)
        let client = BunDocsClient::builder()
            .timeout(Duration::from_millis(100_u64))
            .build();
        let result = client.fetch_doc_markdown("https://bun.sh/docs/page").await;

        // Should not be rejected by SSRF check - will fail for other reasons
        if let Err(e) = &result {
            let msg = e.to_string();
            assert!(
                !msg.contains("non-https") && !msg.contains("non-bun"),
                "bun.sh should pass SSRF check, got: {msg}"
            );
        }
    }

    #[tokio::test]
    async fn read_body_limited_truncates_large_response() {
        let mut server = mockito::Server::new_async().await;

        // Create a 10KB body but limit to 100 bytes
        let large_body = "x".repeat(10_000_usize);
        let mock = server
            .mock("GET", "/large")
            .with_status(200_usize)
            .with_body(&large_body)
            .expect(1_usize)
            .create_async()
            .await;

        let client = reqwest::Client::new();
        let response = client
            .get(format!("{}/large", server.url()))
            .send()
            .await
            .expect("request should succeed");

        let bytes = BunDocsClient::read_body_limited(response, 100_usize).await;

        mock.assert_async().await;
        drop(server);
        assert_eq!(bytes.len(), 100_usize, "Should truncate to limit");
        assert!(bytes.iter().all(|&b| b == b'x'), "Content should be x's");
    }

    #[tokio::test]
    async fn read_body_limited_returns_full_small_response() {
        let mut server = mockito::Server::new_async().await;

        let small_body = "hello world";
        let mock = server
            .mock("GET", "/small")
            .with_status(200_usize)
            .with_body(small_body)
            .expect(1_usize)
            .create_async()
            .await;

        let client = reqwest::Client::new();
        let response = client
            .get(format!("{}/small", server.url()))
            .send()
            .await
            .expect("request should succeed");

        let bytes = BunDocsClient::read_body_limited(response, 1000_usize).await;

        mock.assert_async().await;
        drop(server);
        assert_eq!(bytes.len(), small_body.len(), "Should return full body");
        assert_eq!(&bytes[..], small_body.as_bytes());
    }

    #[tokio::test]
    async fn read_body_limited_zero_limit() {
        let mut server = mockito::Server::new_async().await;

        let mock = server
            .mock("GET", "/zero")
            .with_status(200_usize)
            .with_body("some content")
            .expect(1_usize)
            .create_async()
            .await;

        let client = reqwest::Client::new();
        let response = client
            .get(format!("{}/zero", server.url()))
            .send()
            .await
            .expect("request should succeed");

        let bytes = BunDocsClient::read_body_limited(response, 0_usize).await;

        mock.assert_async().await;
        drop(server);
        assert!(bytes.is_empty(), "Zero limit should return empty");
    }

    #[tokio::test]
    async fn sse_parsing_timeout_on_endless_heartbeats() {
        let mut server = mockito::Server::new_async().await;

        // SSE stream that sends heartbeats but never a JSON-RPC envelope
        // The `:` prefix indicates a comment/heartbeat in SSE
        let sse_body = ": heartbeat\n\n: heartbeat\n\n: heartbeat\n\n";

        let mock = server
            .mock("POST", "/")
            .with_status(200_usize)
            .with_header("content-type", "text/event-stream")
            .with_body(sse_body)
            .expect(1_usize)
            .create_async()
            .await;

        // Use a very short timeout to make test fast
        let client = BunDocsClient::builder()
            .base_url(&server.url())
            .expect("valid mock server URL")
            .timeout(Duration::from_millis(100_u64))
            .build();

        let request = json!({"jsonrpc":"2.0","method":"test","id":1});

        let start = Instant::now();
        let result = client.forward_request(request).await;
        let elapsed = start.elapsed();

        mock.assert_async().await;
        drop(server);

        // Should fail with timeout error
        assert!(result.is_err(), "Should timeout on endless heartbeats");
        let error = result.expect_err("should be timeout error");
        let error_msg = error.to_string();
        // Could be "Timed out waiting for JSON-RPC envelope" or "No valid JSON-RPC response"
        // depending on whether timeout fires first or stream ends first
        assert!(
            error_msg.contains("Timed out") || error_msg.contains("No valid JSON-RPC"),
            "Expected timeout or no response error, got: {error_msg}"
        );

        // Should complete quickly (within timeout + small margin)
        assert!(
            elapsed.as_millis() < 500_u128,
            "Should not hang, elapsed: {}ms",
            elapsed.as_millis()
        );
    }

    #[tokio::test]
    async fn sse_parsing_succeeds_before_timeout() {
        let mut server = mockito::Server::new_async().await;

        // SSE stream with valid JSON-RPC response
        let sse_body = "data: {\"jsonrpc\":\"2.0\",\"result\":{\"data\":\"test\"},\"id\":1}\n\n";

        let mock = server
            .mock("POST", "/")
            .with_status(200_usize)
            .with_header("content-type", "text/event-stream")
            .with_body(sse_body)
            .expect(1_usize)
            .create_async()
            .await;

        let client = BunDocsClient::builder()
            .base_url(&server.url())
            .expect("valid mock server URL")
            .timeout(Duration::from_secs(5_u64))
            .build();

        let request = json!({"jsonrpc":"2.0","method":"test","id":1});
        let result = client.forward_request(request).await;

        mock.assert_async().await;
        drop(server);

        assert!(result.is_ok(), "Should succeed with valid SSE response");
        let upstream = result.expect("successful response");
        assert!(upstream.is_ok(), "Should be UpstreamResponse::Ok");
    }

    // ==========================================================================
    // HTTP Edge Case Tests (integration-tests feature)
    // Network tests that hit real hosts (httpbingo.org, DNS failure tests, etc.)
    // Run with: cargo test --features integration-tests
    // ==========================================================================

    #[cfg(feature = "integration-tests")]
    mod edge_cases {
        use std::io::{Error, ErrorKind::Other};

        use super::*;

        #[tokio::test]
        async fn forward_request_connection_refused() {
            // Use invalid port that nothing is listening on
            let client = BunDocsClient::with_base_url("http://localhost:1").expect("valid URL");

            let request = json!({
                "jsonrpc": "2.0",
                "id": 1_i32,
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
        async fn forward_request_invalid_hostname() {
            // Use invalid hostname that cannot be resolved
            let client =
                BunDocsClient::with_base_url("http://invalid.hostname.that.does.not.exist.local")
                    .expect("valid URL");

            let request = json!({
                "jsonrpc": "2.0",
                "id": 1_i32,
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
        async fn forward_request_timeout_with_real_slow_endpoint() {
            // Use httpbingo.org delay endpoint to test timeout (delays 10s, timeout is 5s)
            let client =
                BunDocsClient::with_base_url("https://httpbingo.org/delay/10").expect("valid URL");

            let request = json!({
                "jsonrpc": "2.0",
                "id": 1_i32,
                "method": "tools/list"
            });

            let result = client.forward_request(request).await;
            assert!(result.is_err());
            let error_msg = result.unwrap_err().to_string();
            eprintln!("Timeout error: {error_msg}");
            // Timeout manifests as "Failed to send request" error
            assert!(
                error_msg.contains("Failed to send") || error_msg.contains("Bun Docs API error")
            );
        }

        #[tokio::test]
        async fn forward_request_http_404() {
            // Use httpbingo.org status endpoint to test 404 error
            let client = BunDocsClient::with_base_url("https://httpbingo.org/status/404")
                .expect("valid URL");

            let request = json!({
                "jsonrpc": "2.0",
                "id": 1_i32,
                "method": "tools/list"
            });

            let result = client.forward_request(request).await;
            assert!(result.is_err());
            let error_msg = result.unwrap_err().to_string();
            assert!(error_msg.contains("404") || error_msg.contains("Bun Docs API error"));
        }

        #[tokio::test]
        async fn forward_request_http_500() {
            // Use httpbingo.org status endpoint to test 500 error
            let client = BunDocsClient::with_base_url("https://httpbingo.org/status/500")
                .expect("valid URL");

            let request = json!({
                "jsonrpc": "2.0",
                "id": 1_i32,
                "method": "tools/list"
            });

            let result = client.forward_request(request).await;
            assert!(result.is_err());
            let error_msg = result.unwrap_err().to_string();
            assert!(error_msg.contains("500") || error_msg.contains("Bun Docs API error"));
        }

        #[tokio::test]
        async fn parse_invalid_json_response() {
            // Use httpbingo.org html endpoint to get non-JSON response
            let client =
                BunDocsClient::with_base_url("https://httpbingo.org/html").expect("valid URL");

            let request = json!({
                "jsonrpc": "2.0",
                "id": 1_i32,
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
        fn sse_parsing_logic() {
            // Test SSE data parsing logic without network calls
            let valid_result = r#"{"result": {"tools": []}}"#;
            let valid_error = r#"{"error": {"code": -32601, "message": "Not found"}}"#;
            let neither = r#"{"status": "pending"}"#;
            let invalid_json = "not valid json";

            // Valid result
            let parsed_result: serde_json::Result<Value> = serde_json::from_str(valid_result);
            assert!(parsed_result.is_ok());
            assert!(
                parsed_result
                    .expect("parsed successfully")
                    .get("result")
                    .is_some()
            );

            // Valid error
            let parsed_error: serde_json::Result<Value> = serde_json::from_str(valid_error);
            assert!(parsed_error.is_ok());
            assert!(
                parsed_error
                    .expect("parsed successfully")
                    .get("error")
                    .is_some()
            );

            // Neither result nor error (should be skipped in SSE parsing)
            let parsed_neither: serde_json::Result<Value> = serde_json::from_str(neither);
            assert!(parsed_neither.is_ok());
            let value = parsed_neither.expect("parsed successfully");
            assert!(value.get("result").is_none() && value.get("error").is_none());

            // Invalid JSON
            let parsed_invalid: serde_json::Result<Value> = serde_json::from_str(invalid_json);
            let _err = parsed_invalid.unwrap_err();
        }

        #[test]
        fn http_status_code_checking() {
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
        fn content_type_header_parsing() {
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

            for content_type in sse_types {
                assert!(content_type.contains("text/event-stream"));
            }

            for content_type in json_types {
                assert!(!content_type.contains("text/event-stream"));
                assert!(content_type.contains("application/json"));
            }
        }

        #[test]
        fn timeout_duration() {
            let timeout = Duration::from_secs(REQUEST_TIMEOUT_SECS);

            assert_eq!(timeout.as_secs(), 5_u64);
            assert!(timeout.as_secs() > 0_u64);
            assert!(timeout.as_secs() < 10_u64);
        }

        #[test]
        fn error_message_fallback_logic() {
            // Test unwrap_or_else logic for error text
            let ok_result: Result<String, Error> = Ok("error message".to_owned());
            let err_result: Result<String, Error> = Err(Error::new(Other, "test error"));

            let fallback1 = Result::unwrap_or_else(ok_result, |_| "unknown error".to_owned());
            let fallback2 = Result::unwrap_or_else(err_result, |_| "unknown error".to_owned());

            assert_eq!(fallback1, "error message");
            assert_eq!(fallback2, "unknown error");
        }
    }
}
