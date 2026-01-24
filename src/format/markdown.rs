//! Markdown output formatter with MDX fetching.

use super::extract_doc_entries;
use crate::upstream::bun_docs::BunDocsClient;
use anyhow::Result;
use core::fmt::Write as _;
use tracing::warn;

/// Formats a search result as a Markdown string by fetching the raw MDX content from URLs.
///
/// This function extracts `DocEntry` items from the search result. For each entry with a URL,
/// it attempts to fetch the full MDX content. If successful, the content is included with a source
/// comment. If the fetch fails or no URL is present, it falls back to the entry's text.
/// The final output joins all parts with Markdown horizontal rules.
///
/// # Arguments
/// * `result` - A reference to the `serde_json::Value` representing the search result.
/// * `client` - A reference to the `BunDocsClient` for fetching MDX content.
///
/// # Returns
/// A `Result` containing the aggregated and formatted Markdown string.
///
/// # Errors
///
/// Returns an error if JSON serialization fails during fallback formatting.
pub async fn format_markdown(result: &serde_json::Value, client: &BunDocsClient) -> Result<String> {
    let doc_entries = extract_doc_entries(result);

    if doc_entries.is_empty() {
        // No content found, fallback to JSON display
        let mut output = String::new();
        output.push_str("```json\n");
        output.push_str(&serde_json::to_string_pretty(result)?);
        output.push_str("\n```\n");
        return Ok(output);
    }

    let mut mdx_parts = Vec::new();

    for entry in doc_entries {
        if let Some(url) = entry.url {
            // Try to fetch MDX from the URL
            let fetch_result = client.fetch_doc_markdown(&url).await;
            match fetch_result {
                Ok(mdx) => {
                    // Success: include URL comment and MDX content
                    let mut part = String::new();
                    write!(part, "<!-- Source: {url} -->\n\n")?;
                    part.push_str(&mdx);
                    mdx_parts.push(part);
                }
                Err(e) => {
                    // Error: include error comment and fallback to original text
                    warn!("Failed to fetch MDX from {url}: {e}");
                    let mut part = String::new();
                    write!(part, "<!-- Error: {e} -->\n\n")?;
                    part.push_str(entry.text);
                    mdx_parts.push(part);
                }
            }
        } else {
            // No URL found, use original text content
            mdx_parts.push(entry.text.to_owned());
        }
    }

    // Join with horizontal rules and two newlines
    Ok(mdx_parts.join("\n\n---\n\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_format_markdown_no_url() {
        // Test content without URL - should just return the text
        let result = json!({"content": [{"text": "test content", "type": "text"}]});
        let client = BunDocsClient::new();
        let formatted = format_markdown(&result, &client).await.unwrap();
        assert!(formatted.contains("test content"));
        assert!(!formatted.contains("<!--")); // No URL comment
    }

    #[tokio::test]
    async fn test_format_markdown_no_content() {
        // Test fallback to JSON when no content array
        let result = json!({"other": "data"});
        let client = BunDocsClient::new();
        let formatted = format_markdown(&result, &client).await.unwrap();
        assert!(formatted.contains("```json"));
        assert!(formatted.contains("\"other\""));
    }

    #[tokio::test]
    async fn test_format_markdown_multiple_items_no_url() {
        // Test multiple items without URLs
        let result = json!({"content": [
            {"text": "First Section", "type": "text"},
            {"text": "Second Section", "type": "text"}
        ]});
        let client = BunDocsClient::new();
        let formatted = format_markdown(&result, &client).await.unwrap();
        assert!(formatted.contains("First Section"));
        assert!(formatted.contains("Second Section"));
        assert!(formatted.contains("\n\n---\n\n")); // Horizontal rule separator
    }

    #[tokio::test]
    async fn test_format_markdown_empty_content() {
        // Test empty content array falls back to JSON
        let result = json!({"content": []});
        let client = BunDocsClient::new();
        let formatted = format_markdown(&result, &client).await.unwrap();
        assert!(formatted.contains("```json"));
        assert!(formatted.contains("\"content\": []"));
    }

    #[tokio::test]
    async fn test_format_markdown_with_null_content() {
        let result = json!({"content": null});
        let client = BunDocsClient::new();
        let formatted = format_markdown(&result, &client).await.unwrap();
        assert!(formatted.contains("```json"));
        assert!(formatted.contains("null"));
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

        let result = json!({"content": [{
            "text": format!("Original text content\nLink: {}/docs/page", server.url()),
            "type": "text"
        }]});

        let client = BunDocsClient::with_base_url(&server.url()).expect("valid URL");
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
        let result = json!({"content": [{
            "text": format!("Summary\nLink: {url}"),
            "type": "text"
        }]});

        let client = BunDocsClient::with_base_url(&server.url()).expect("valid mock server URL");
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
    }
}
