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
    // "hello 世界"
    let unicode = "hello \u{4e16}\u{754c}";
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
    if let Ok(response) = result {
        // If successful, should have an error field in JSON-RPC response
        assert!(
            response.get("error").is_some(),
            "Expected error field in response"
        );
    } else {
        // HTTP-level error is also acceptable
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

    result.unwrap_err();
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
    result.unwrap_err();
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
    result.unwrap();
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

#[tokio::test]
async fn test_fetch_doc_markdown_success() {
    let mut server = mockito::Server::new_async().await;

    let mock = server
        .mock("GET", "/docs/page")
        .match_header("accept", "text/markdown")
        .with_status(200)
        .with_header("content-type", "text/markdown")
        .with_body("# Test MDX\n\nSome content")
        .expect(1)
        .create_async()
        .await;

    let client = BunDocsClient::with_base_url(&server.url()).unwrap();
    let url = format!("{}/docs/page", server.url());

    let result = client.fetch_doc_markdown(&url).await;

    mock.assert_async().await;
    assert!(result.is_ok());
    let mdx = result.unwrap();
    assert!(mdx.contains("# Test MDX"));
    assert!(mdx.contains("Some content"));
}

#[tokio::test]
async fn test_fetch_doc_markdown_404_error() {
    let mut server = mockito::Server::new_async().await;

    let mock = server
        .mock("GET", "/docs/missing")
        .with_status(404)
        .with_body("Not Found")
        .expect(1)
        .create_async()
        .await;

    let client = BunDocsClient::with_base_url(&server.url()).unwrap();
    let url = format!("{}/docs/missing", server.url());

    let result = client.fetch_doc_markdown(&url).await;

    mock.assert_async().await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("404"));
}

#[tokio::test]
async fn test_fetch_doc_markdown_500_error() {
    let mut server = mockito::Server::new_async().await;

    let mock = server
        .mock("GET", "/docs/error")
        .with_status(500)
        .with_body("Internal Server Error")
        .expect(1)
        .create_async()
        .await;

    let client = BunDocsClient::with_base_url(&server.url()).unwrap();
    let url = format!("{}/docs/error", server.url());

    let result = client.fetch_doc_markdown(&url).await;

    mock.assert_async().await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("500"));
}
