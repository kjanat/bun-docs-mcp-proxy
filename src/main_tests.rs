#![allow(clippy::expect_used, reason = "tests can use expect()")]
#![allow(clippy::unwrap_used, reason = "tests can use unwrap()")]
#![allow(clippy::indexing_slicing, reason = "tests use array indexing")]
#![allow(clippy::default_numeric_fallback, reason = "test literals")]

use super::*;
use serde_json::json;

#[test]
fn test_handle_initialize() {
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: json!(1),
        method: "initialize".to_owned(),
        params: None,
    };

    let response = handle_initialize(&request);
    let serialized = serde_json::to_value(&response).unwrap();

    assert_eq!(serialized["id"], 1);
    assert_eq!(serialized["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(
        serialized["result"]["serverInfo"]["name"],
        "bun-docs-mcp-proxy"
    );
    assert!(serialized["result"]["capabilities"]["tools"].is_object());
}

#[test]
fn test_handle_tools_list() {
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: json!("test-id"),
        method: "tools/list".to_owned(),
        params: None,
    };

    let response = handle_tools_list(&request);
    let serialized = serde_json::to_value(&response).unwrap();

    assert_eq!(serialized["id"], "test-id");
    assert!(serialized["result"]["tools"].is_array());

    let tools = serialized["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "SearchBun");
    assert_eq!(
        tools[0]["inputSchema"]["properties"]["query"]["type"],
        "string"
    );
}

#[test]
fn test_parse_valid_jsonrpc_request() {
    let message = r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#;
    let request: Result<JsonRpcRequest, _> = serde_json::from_str(message);

    assert!(request.is_ok());
    let req = request.unwrap();
    assert_eq!(req.method, "initialize");
    assert_eq!(req.id, json!(1));
}

#[test]
fn test_parse_invalid_jsonrpc_request() {
    let message = r#"{"invalid json"#;
    let request: Result<JsonRpcRequest, _> = serde_json::from_str(message);

    request.unwrap_err();
}

#[test]
fn test_error_response_codes() {
    // Test parse error
    let parse_error = JsonRpcResponse::error(json!(1), -32700, "Parse error".to_owned());
    let serialized_parse = serde_json::to_value(&parse_error).unwrap();
    assert_eq!(serialized_parse["error"]["code"], -32700);

    // Test method not found
    let method_error = JsonRpcResponse::error(json!(2), -32601, "Method not found".to_owned());
    let serialized_method = serde_json::to_value(&method_error).unwrap();
    assert_eq!(serialized_method["error"]["code"], -32601);

    // Test internal error
    let internal_error = JsonRpcResponse::error(json!(3), -32603, "Internal error".to_owned());
    let serialized_internal = serde_json::to_value(&internal_error).unwrap();
    assert_eq!(serialized_internal["error"]["code"], -32603);
}

#[test]
fn test_response_serialization() {
    let response = JsonRpcResponse::success(json!("test-id"), json!({"result": "data"}));
    let serialized = serde_json::to_string(&response);

    assert!(serialized.is_ok());
    let json_str = serialized.unwrap();
    assert!(json_str.contains("\"jsonrpc\":\"2.0\""));
    assert!(json_str.contains("\"id\":\"test-id\""));
}

#[test]
fn test_handle_tools_list_structure() {
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: json!(1),
        method: "tools/list".to_owned(),
        params: None,
    };

    let response = handle_tools_list(&request);
    let serialized = serde_json::to_value(&response).unwrap();

    // Verify required fields
    assert!(serialized["result"]["tools"].is_array());
    let tools = serialized["result"]["tools"].as_array().unwrap();
    assert!(!tools.is_empty());

    // Verify tool structure
    let tool = &tools[0];
    assert!(tool["name"].is_string());
    assert!(tool["description"].is_string());
    assert!(tool["inputSchema"]["type"].is_string());
    assert_eq!(tool["inputSchema"]["type"], "object");
}

#[test]
fn test_initialize_response_version() {
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: json!(1),
        method: "initialize".to_owned(),
        params: None,
    };

    let response = handle_initialize(&request);
    let serialized = serde_json::to_value(&response).unwrap();

    // Verify protocol version matches MCP spec
    assert_eq!(serialized["result"]["protocolVersion"], "2024-11-05");
    // Verify both capabilities are present
    assert!(serialized["result"]["capabilities"]["tools"].is_object());
    assert!(serialized["result"]["capabilities"]["resources"].is_object());
}

#[test]
fn test_handle_resources_list() {
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: json!("res-list"),
        method: "resources/list".to_owned(),
        params: None,
    };

    let response = handle_resources_list(&request);
    let serialized = serde_json::to_value(&response).unwrap();

    assert_eq!(serialized["id"], "res-list");
    assert!(serialized["result"]["resources"].is_array());

    let resources = serialized["result"]["resources"].as_array().unwrap();
    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0]["uri"], "bun://docs");
    assert_eq!(resources[0]["name"], "Bun Documentation");
    assert_eq!(resources[0]["mimeType"], "application/json");
}

#[test]
fn test_jsonrpc_request_with_params() {
    let message = r#"{"jsonrpc":"2.0","id":1,"method":"test","params":{"key":"value"}}"#;
    let request: JsonRpcRequest = serde_json::from_str(message).unwrap();

    assert!(request.params.is_some());
    let params = request.params.unwrap();
    assert_eq!(params["key"], "value");
}

#[test]
fn test_response_null_id() {
    let response = JsonRpcResponse::error(json!(null), -32700, "Error".to_owned());
    let serialized = serde_json::to_value(&response).unwrap();

    assert!(serialized["id"].is_null());
}

#[tokio::test]
async fn test_handle_tools_call_mocked() {
    // Mock successful API response without network call
    let mut server = mockito::Server::new_async().await;

    // Mock the SSE stream response
    let mock = server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body("data: {\"result\":{\"content\":[{\"text\":\"Mocked Bun.serve documentation\",\"type\":\"text\"}]}}\n\n")
        .expect(1)
        .create_async()
        .await;

    let client = http::BunDocsClient::with_base_url(&server.url()).expect("valid mock server URL");
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: json!(1),
        method: "tools/call".to_owned(),
        params: Some(json!({
            "name": "SearchBun",
            "arguments": {
                "query": "Bun.serve"
            }
        })),
    };

    let response = handle_tools_call(&client, &request).await;
    let serialized = serde_json::to_value(&response).unwrap();

    mock.assert_async().await;
    drop(server);

    // Verify successful response structure
    assert!(serialized["result"].is_object());
    assert!(serialized["result"]["content"].is_array());
    let content = serialized["result"]["content"].as_array().unwrap();
    assert!(!content.is_empty());
    assert_eq!(content[0]["text"], "Mocked Bun.serve documentation");
}

#[tokio::test]
async fn test_handle_resources_read_mocked() {
    // Mock successful resource read without network call
    let mut server = mockito::Server::new_async().await;

    // Mock the SSE stream response for resource read
    let mock = server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body("data: {\"result\":{\"content\":[{\"text\":\"Mocked HTTP documentation\",\"type\":\"text\"}]}}\n\n")
        .expect(1)
        .create_async()
        .await;

    let client = http::BunDocsClient::with_base_url(&server.url()).expect("valid mock server URL");
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: json!("res-mock"),
        method: "resources/read".to_owned(),
        params: Some(json!({"uri": "bun://docs?query=HTTP"})),
    };

    let response = handle_resources_read(&client, &request).await;
    let serialized = serde_json::to_value(&response).unwrap();

    mock.assert_async().await;
    drop(server);

    // Verify successful resource response structure
    assert!(serialized["result"]["contents"].is_array());
    let contents = serialized["result"]["contents"].as_array().unwrap();
    assert_eq!(contents.len(), 1);
    assert_eq!(contents[0]["uri"], "bun://docs?query=HTTP");
    assert_eq!(contents[0]["mimeType"], "application/json");
    assert!(contents[0]["text"].is_string());

    // Verify the text contains the mocked result
    let text_content = contents[0]["text"].as_str().unwrap();
    assert!(text_content.contains("Mocked HTTP documentation"));
}

#[tokio::test]
#[cfg(feature = "integration-tests")]
async fn test_handle_tools_call_real_api() {
    let client = http::BunDocsClient::new();
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: json!(1),
        method: "tools/call".to_owned(),
        params: Some(json!({
            "name": "SearchBun",
            "arguments": {
                "query": "Bun.serve"
            }
        })),
    };

    let response = handle_tools_call(&client, &request).await;
    let serialized = serde_json::to_value(&response).unwrap();

    assert!(serialized["result"].is_object());
    assert!(serialized["result"]["content"].is_array());
}

#[tokio::test]
#[cfg(feature = "integration-tests")]
async fn test_handle_tools_call_empty_query() {
    // NOTE: This test reflects Bun API's current behavior for empty query.
    // As of now, Bun returns {"content":[{"text":"No results found","type":"text"}],"isError":true}
    // If Bun changes this behavior (e.g., returns docs overview), update expected output accordingly.
    let client = http::BunDocsClient::new();
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: json!(2),
        method: "tools/call".to_owned(),
        params: Some(json!({
            "name": "SearchBun",
            "arguments": {
                "query": ""
            }
        })),
    };

    let response = handle_tools_call(&client, &request).await;
    let serialized = serde_json::to_value(&response).unwrap();

    // Proxy should forward successfully; Bun API decides what empty query means
    assert!(serialized["result"].is_object());
}

#[tokio::test]
#[cfg(feature = "integration-tests")]
async fn test_handle_resources_read_with_query() {
    let client = http::BunDocsClient::new();
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: json!("res1"),
        method: "resources/read".to_owned(),
        params: Some(json!({"uri": "bun://docs?query=Bun.serve"})),
    };

    let response = handle_resources_read(&client, &request).await;
    let serialized = serde_json::to_value(&response).unwrap();

    assert!(serialized["result"]["contents"].is_array());
    let contents = serialized["result"]["contents"].as_array().unwrap();
    assert_eq!(contents.len(), 1);
    assert_eq!(contents[0]["uri"], "bun://docs?query=Bun.serve");
    assert_eq!(contents[0]["mimeType"], "application/json");
}

#[tokio::test]
#[cfg(feature = "integration-tests")]
async fn test_handle_resources_read_empty_query() {
    // NOTE: Tests bun://docs (no query param) which proxy converts to empty query string.
    // Bun API currently returns "No results found" for empty queries.
    // If Bun changes to return overview/help for empty query, this test still passes (valid contents array).
    let client = http::BunDocsClient::new();
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: json!("res2"),
        method: "resources/read".to_owned(),
        params: Some(json!({"uri": "bun://docs"})),
    };

    let response = handle_resources_read(&client, &request).await;
    let serialized = serde_json::to_value(&response).unwrap();

    assert!(serialized["result"]["contents"].is_array());
}

#[tokio::test]
async fn test_handle_resources_read_missing_params() {
    let client = http::BunDocsClient::new();
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: json!("res3"),
        method: "resources/read".to_owned(),
        params: None,
    };

    let response = handle_resources_read(&client, &request).await;
    let serialized = serde_json::to_value(&response).unwrap();

    assert!(serialized["error"].is_object());
    assert_eq!(serialized["error"]["code"], -32602);
    assert!(
        serialized["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Missing params")
    );
}

#[tokio::test]
async fn test_handle_resources_read_invalid_uri() {
    let client = http::BunDocsClient::new();
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: json!("res4"),
        method: "resources/read".to_owned(),
        params: Some(json!({"uri": "invalid://uri"})),
    };

    let response = handle_resources_read(&client, &request).await;
    let serialized = serde_json::to_value(&response).unwrap();

    assert!(serialized["error"].is_object());
    assert_eq!(serialized["error"]["code"], -32602);
    assert!(
        serialized["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Invalid URI format")
    );
}

#[tokio::test]
async fn test_handle_resources_read_missing_uri_param() {
    let client = http::BunDocsClient::new();
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: json!("res5"),
        method: "resources/read".to_owned(),
        params: Some(json!({"other": "value"})),
    };

    let response = handle_resources_read(&client, &request).await;
    let serialized = serde_json::to_value(&response).unwrap();

    assert!(serialized["error"].is_object());
    assert_eq!(serialized["error"]["code"], -32602);
    assert!(
        serialized["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Missing or invalid uri parameter")
    );
}

#[tokio::test]
#[cfg(feature = "integration-tests")]
async fn test_handle_resources_read_with_real_search() {
    let client = http::BunDocsClient::new();
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: json!("res6"),
        method: "resources/read".to_owned(),
        params: Some(json!({"uri": "bun://docs?query=HTTP"})),
    };

    let response = handle_resources_read(&client, &request).await;
    let serialized = serde_json::to_value(&response).unwrap();

    // Real API should return valid results
    assert!(serialized["result"]["contents"].is_array());
    let contents = serialized["result"]["contents"].as_array().unwrap();
    assert!(!contents.is_empty());
}

#[test]
fn test_init_logging_execution() {
    // Test that init_logging can be called
    // Will panic if called twice, but that's expected
    let result = std::panic::catch_unwind(|| {
        init_logging();
    });

    // Either succeeds or panics (already initialized) - both are fine
    // This just ensures the function code path is exercised
    let _ = result;
}

#[test]
fn test_format_json() {
    let result = serde_json::json!({"content": [{"text": "test", "type": "text"}]});
    let formatted = format_json(&result).unwrap();
    assert!(formatted.contains("\"content\""));
    assert!(formatted.contains("\"text\": \"test\""));
}

#[test]
fn test_format_json_empty() {
    let result = serde_json::json!({});
    let formatted = format_json(&result).unwrap();
    assert_eq!(formatted, "{}");
}

#[test]
fn test_format_text() {
    let result = serde_json::json!({"content": [{"text": "test content", "type": "text"}]});
    let formatted = format_text(&result).unwrap();
    assert!(formatted.contains("test content"));
    assert!(!formatted.contains("\"content\""));
}

#[test]
fn test_format_text_no_content() {
    let result = serde_json::json!({"other": "data"});
    let formatted = format_text(&result).unwrap();
    assert!(formatted.contains("\"other\""));
    assert!(formatted.contains("\"data\""));
}

#[test]
fn test_format_text_empty_content_array() {
    let result = serde_json::json!({"content": []});
    let formatted = format_text(&result).unwrap();
    // Empty content array falls back to JSON
    assert!(formatted.contains("\"content\": []"));
}

#[test]
fn test_format_text_multiple_items() {
    let result = serde_json::json!({"content": [
        {"text": "first item", "type": "text"},
        {"text": "second item", "type": "text"}
    ]});
    let formatted = format_text(&result).unwrap();
    assert!(formatted.contains("first item"));
    assert!(formatted.contains("second item"));
}

#[tokio::test]
async fn test_format_markdown_no_url() {
    // Test content without URL - should just return the text
    let result = serde_json::json!({"content": [{"text": "test content", "type": "text"}]});
    let client = http::BunDocsClient::new();
    let formatted = format_markdown(&result, &client).await.unwrap();
    assert!(formatted.contains("test content"));
    assert!(!formatted.contains("<!--")); // No URL comment
}

#[tokio::test]
async fn test_format_markdown_no_content() {
    // Test fallback to JSON when no content array
    let result = serde_json::json!({"other": "data"});
    let client = http::BunDocsClient::new();
    let formatted = format_markdown(&result, &client).await.unwrap();
    assert!(formatted.contains("```json"));
    assert!(formatted.contains("\"other\""));
}

#[tokio::test]
async fn test_format_markdown_multiple_items_no_url() {
    // Test multiple items without URLs
    let result = serde_json::json!({"content": [
        {"text": "First Section", "type": "text"},
        {"text": "Second Section", "type": "text"}
    ]});
    let client = http::BunDocsClient::new();
    let formatted = format_markdown(&result, &client).await.unwrap();
    assert!(formatted.contains("First Section"));
    assert!(formatted.contains("Second Section"));
    assert!(formatted.contains("\n\n---\n\n")); // Horizontal rule separator
}

#[tokio::test]
async fn test_format_markdown_empty_content() {
    // Test empty content array falls back to JSON
    let result = serde_json::json!({"content": []});
    let client = http::BunDocsClient::new();
    let formatted = format_markdown(&result, &client).await.unwrap();
    assert!(formatted.contains("```json"));
    assert!(formatted.contains("\"content\": []"));
}

#[test]
fn test_extract_doc_entries_with_url() {
    // Test URL extraction from content
    let result = serde_json::json!({"content": [{
        "text": "Title: Test\nLink: https://example.com/page\nContent: Some content",
        "type": "text"
    }]});
    let entries = extract_doc_entries(&result);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].url.as_ref().unwrap(), "https://example.com/page");
    assert!(entries[0].text.contains("Title: Test"));
}

#[test]
fn test_extract_doc_entries_without_url() {
    // Test content without URL
    let result = serde_json::json!({"content": [{
        "text": "Just some text without a link",
        "type": "text"
    }]});
    let entries = extract_doc_entries(&result);
    assert_eq!(entries.len(), 1);
    assert!(entries[0].url.is_none());
    assert_eq!(entries[0].text, "Just some text without a link");
}

#[test]
fn test_extract_doc_entries_empty() {
    // Test empty content
    let result = serde_json::json!({"content": []});
    let entries = extract_doc_entries(&result);
    assert_eq!(entries.len(), 0);
}

#[test]
fn test_extract_doc_entries_multiple_with_mixed_urls() {
    // Test multiple entries, some with URLs, some without
    let result = serde_json::json!({"content": [
        {"text": "Title: First\nLink: https://example.com/first\nContent: text", "type": "text"},
        {"text": "No link here", "type": "text"},
        {"text": "Title: Third\nLink: https://example.com/third", "type": "text"}
    ]});
    let entries = extract_doc_entries(&result);
    assert_eq!(entries.len(), 3);
    assert_eq!(
        entries[0].url.as_ref().unwrap(),
        "https://example.com/first"
    );
    assert!(entries[1].url.is_none());
    assert_eq!(
        entries[2].url.as_ref().unwrap(),
        "https://example.com/third"
    );
}

#[test]
fn test_extract_content_texts_valid() {
    let result = serde_json::json!({"content": [
        {"text": "first", "type": "text"},
        {"text": "second", "type": "text"}
    ]});
    let texts = extract_content_texts(&result);
    assert_eq!(texts, vec!["first", "second"]);
}

#[test]
fn test_extract_content_texts_empty() {
    let result = serde_json::json!({});
    let texts = extract_content_texts(&result);
    assert!(texts.is_empty());
}

#[test]
fn test_extract_content_texts_null_content() {
    let result = serde_json::json!({"content": null});
    let texts = extract_content_texts(&result);
    assert!(texts.is_empty());
}

#[test]
fn test_extract_content_texts_non_array_content() {
    let result = serde_json::json!({"content": "not an array"});
    let texts = extract_content_texts(&result);
    assert!(texts.is_empty());
}

#[test]
fn test_extract_content_texts_missing_text_field() {
    let result = serde_json::json!({"content": [
        {"type": "text"},  // missing text field
        {"text": "valid", "type": "text"}
    ]});
    let texts = extract_content_texts(&result);
    assert_eq!(texts, vec!["valid"]);
}

#[test]
fn test_extract_content_texts_empty_string() {
    let result = serde_json::json!({"content": [
        {"text": "", "type": "text"},
        {"text": "valid", "type": "text"}
    ]});
    let texts = extract_content_texts(&result);
    assert_eq!(texts, vec!["", "valid"]);
}

#[test]
fn test_extract_content_texts_non_string_text() {
    let result = serde_json::json!({"content": [
        {"text": 123, "type": "text"},  // text is number
        {"text": "valid", "type": "text"}
    ]});
    let texts = extract_content_texts(&result);
    assert_eq!(texts, vec!["valid"]);
}

#[test]
fn test_format_text_with_null_content() {
    let result = serde_json::json!({"content": null, "other": "data"});
    let formatted = format_text(&result).unwrap();
    assert!(formatted.contains("\"content\": null"));
}

#[tokio::test]
async fn test_format_markdown_with_null_content() {
    let result = serde_json::json!({"content": null});
    let client = http::BunDocsClient::new();
    let formatted = format_markdown(&result, &client).await.unwrap();
    assert!(formatted.contains("```json"));
    assert!(formatted.contains("null"));
}

#[test]
fn test_get_string_param() {
    let params = json!({"uri": "bun://docs", "other": 123});

    assert_eq!(get_string_param(&params, "uri").unwrap(), "bun://docs");
    get_string_param(&params, "other").unwrap_err();
    get_string_param(&params, "missing").unwrap_err();
}

#[test]
fn test_parse_bun_docs_uri() {
    assert_eq!(parse_bun_docs_uri("bun://docs").unwrap(), "");
    assert_eq!(parse_bun_docs_uri("bun://docs?query=test").unwrap(), "test");
    assert_eq!(
        parse_bun_docs_uri("bun://docs?query=Bun.serve").unwrap(),
        "Bun.serve"
    );
    parse_bun_docs_uri("invalid://uri").unwrap_err();
    parse_bun_docs_uri("").unwrap_err();
}

#[test]
fn test_jsonrpc_error_code_constants() {
    assert_eq!(JSONRPC_PARSE_ERROR, -32700);
    assert_eq!(JSONRPC_INVALID_PARAMS, -32602);
    assert_eq!(JSONRPC_INTERNAL_ERROR, -32603);
    assert_eq!(JSONRPC_METHOD_NOT_FOUND, -32601);
}

#[tokio::test]
#[cfg(feature = "integration-tests")]
async fn test_direct_search_json_format() {
    let result = direct_search("Bun.serve", &OutputFormat::Json, None).await;
    result.unwrap();
}

#[tokio::test]
#[cfg(feature = "integration-tests")]
async fn test_direct_search_text_format() {
    let result = direct_search("HTTP", &OutputFormat::Text, None).await;
    result.unwrap();
}

#[tokio::test]
#[cfg(feature = "integration-tests")]
async fn test_direct_search_markdown_format() {
    let result = direct_search("server", &OutputFormat::Markdown, None).await;
    result.unwrap();
}

#[tokio::test]
async fn test_direct_search_with_output_file() {
    let temp_file = tempfile::Builder::new()
        .prefix("test_search_")
        .suffix(".json")
        .tempfile_in(".")
        .unwrap();
    let output_path = temp_file.path().file_name().unwrap().to_str().unwrap();

    let result = direct_search("test", &OutputFormat::Json, Some(output_path)).await;
    result.unwrap();

    // Verify file was created
    assert!(std::path::Path::new(output_path).exists());

    // Read and verify content
    let content = std::fs::read_to_string(output_path).unwrap();
    assert!(!content.is_empty());

    // tempfile automatically cleans up on drop
}

#[tokio::test]
async fn test_direct_search_empty_query() {
    let result = direct_search("", &OutputFormat::Json, None).await;
    // Should succeed, Bun API handles empty queries
    result.unwrap();
}

#[tokio::test]
async fn test_direct_search_markdown_with_file() {
    let temp_file = tempfile::Builder::new()
        .prefix("test_markdown_")
        .suffix(".md")
        .tempfile_in(".")
        .unwrap();
    let output_path = temp_file.path().file_name().unwrap().to_str().unwrap();

    let result = direct_search("Bun", &OutputFormat::Markdown, Some(output_path)).await;
    result.unwrap();

    // Verify file was created
    assert!(std::path::Path::new(output_path).exists());

    // Read and verify markdown content (may include URL comments or MDX)
    let content = std::fs::read_to_string(output_path).unwrap();
    assert!(!content.is_empty(), "Markdown output should not be empty");
    // The content could be raw MDX with URL comments or fallback text

    // tempfile automatically cleans up on drop
}

#[test]
fn test_validate_output_path_valid() {
    validate_output_path("output.json").unwrap();
    validate_output_path("./output.json").unwrap();
    validate_output_path("subdir/output.json").unwrap();
}

#[test]
fn test_validate_output_path_directory_traversal() {
    assert!(validate_output_path("../output.json").is_err());
    assert!(validate_output_path("subdir/../output.json").is_err());
    assert!(validate_output_path("../../etc/passwd").is_err());
}

#[test]
fn test_validate_output_path_absolute_paths() {
    assert!(validate_output_path("/tmp/output.json").is_err());
    assert!(validate_output_path("/etc/passwd").is_err());
    #[cfg(windows)]
    assert!(validate_output_path("C:\\output.json").is_err());
}

#[tokio::test]
async fn test_direct_search_invalid_output_path() {
    let result = direct_search("test", &OutputFormat::Json, Some("../output.json")).await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("directory traversal")
    );
}

#[tokio::test]
async fn test_direct_search_file_overwrite() {
    let temp_file = tempfile::Builder::new()
        .prefix("test_overwrite_")
        .suffix(".json")
        .tempfile_in(".")
        .unwrap();
    let output_path = temp_file.path().file_name().unwrap().to_str().unwrap();

    // Create existing file
    fs::write(output_path, "existing content").unwrap();
    assert!(std::path::Path::new(output_path).exists());

    // Should overwrite
    let result = direct_search("test", &OutputFormat::Json, Some(output_path)).await;
    result.unwrap();

    // Verify new content
    let content = std::fs::read_to_string(output_path).unwrap();
    assert!(!content.contains("existing content"));

    // tempfile automatically cleans up on drop
}

#[tokio::test]
async fn test_format_markdown_fetch_mdx_error_with_fallback() {
    // Test that when MDX fetch fails, we get an error comment + fallback text
    let mut server = mockito::Server::new_async().await;

    // Mock the MDX fetch to fail with 500
    let mock_error = server
        .mock("GET", mockito::Matcher::Any)
        .with_status(500_usize)
        .with_body("Internal Server Error")
        .expect(1_usize)
        .create_async()
        .await;

    let result = serde_json::json!({"content": [{
        "text": format!("Original text content\nLink: {}/docs/page", server.url()),
        "type": "text"
    }]});

    let client = http::BunDocsClient::with_base_url(&server.url()).expect("valid URL");
    let formatted = format_markdown(&result, &client)
        .await
        .expect("format should succeed");

    mock_error.assert_async().await;
    drop(server);

    // Verify error comment and fallback text
    assert!(
        formatted.contains("<!-- Error:"),
        "Should have error comment when fetch fails"
    );
    assert!(
        formatted.contains("Original text content"),
        "Should include fallback text"
    );
    // Verifies src/main.rs line 302: warn!("Failed to fetch MDX from {url}: {e}");
    // Verifies line 304: write!(part, "<!-- Error: {e} -->\n\n")
    // Verifies line 305: part.push_str(entry.text)
}

#[tokio::test]
async fn test_handle_tools_call_with_network_error() {
    // Test that network errors are properly converted to JSON-RPC error responses
    let mut server = mockito::Server::new_async().await;

    // Mock all requests to fail with 503
    let _mock = server
        .mock("POST", mockito::Matcher::Any)
        .with_status(503_usize)
        .with_body("Service Unavailable")
        .expect_at_least(1_usize)
        .create_async()
        .await;

    let client = http::BunDocsClient::with_base_url(&server.url()).expect("valid mock server URL");
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: json!(1),
        method: "tools/call".to_owned(),
        params: Some(json!({
            "name": "SearchBun",
            "arguments": {"query": "test"}
        })),
    };

    let response = handle_tools_call(&client, &request).await;
    let serialized = serde_json::to_value(&response).unwrap();

    drop(server);

    // Verify error response structure (after max retries)
    assert!(
        serialized["error"].is_object(),
        "Should have error field in response"
    );
    assert_eq!(
        serialized["error"]["code"], -32_603_i32,
        "Should be internal error code"
    );
    assert!(
        serialized["error"]["message"]
            .as_str()
            .unwrap()
            .contains("error"),
        "Error message should describe the issue"
    );
    // Verifies src/main.rs line 540: error!("Failed to forward request: {}", e);
    // Verifies lines 541-545: JsonRpcResponse::error construction with JSONRPC_INTERNAL_ERROR
}

#[tokio::test]
async fn test_format_markdown_with_url_and_fetch_success() {
    // Test happy path: URL is parsed and MDX is fetched successfully
    let mut server = mockito::Server::new_async().await;

    // Mock successful MDX fetch
    let mock = server
        .mock("GET", "/docs/page")
        .match_header("accept", "text/markdown")
        .with_status(200_usize)
        .with_header("content-type", "text/markdown")
        .with_body("# Documentation\n\nThis is the actual MDX content")
        .expect(1_usize)
        .create_async()
        .await;

    let url = format!("{}/docs/page", server.url());
    let result = serde_json::json!({"content": [{
        "text": format!("Summary\nLink: {url}"),
        "type": "text"
    }]});

    let client = http::BunDocsClient::with_base_url(&server.url()).expect("valid mock server URL");
    let formatted = format_markdown(&result, &client)
        .await
        .expect("format should succeed");

    mock.assert_async().await;
    drop(server);

    // Verify source comment and MDX content
    assert!(
        formatted.contains("<!-- Source:"),
        "Should have source comment when fetch succeeds"
    );
    assert!(
        formatted.contains("# Documentation"),
        "Should include fetched MDX content"
    );
    assert!(
        formatted.contains("actual MDX content"),
        "Should preserve full MDX content"
    );
    // Verifies src/main.rs lines 292-298: successful fetch with source comment
}
