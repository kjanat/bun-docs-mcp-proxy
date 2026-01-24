//! Markdown output formatter with MDX fetching.

use super::extract_doc_entries;
use crate::upstream::bun_docs::BunDocsClient;
use anyhow::Result;

use futures::stream::{self, StreamExt as _};
use tracing::warn;

/// Maximum number of concurrent MDX fetch requests.
const MAX_CONCURRENT_FETCHES: usize = 4;

/// Formats a search result as a Markdown string by fetching the raw MDX content from URLs.
///
/// This function extracts `DocEntry` items from the search result. For each entry with a URL,
/// it attempts to fetch the full MDX content concurrently (up to 4 parallel requests).
/// If successful, the content is included with a source comment. If the fetch fails or no URL
/// is present, it falls back to the entry's text.
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

    // Fetch MDX content concurrently with index tracking to preserve order
    let indexed_results: Vec<(usize, String)> = stream::iter(doc_entries.into_iter().enumerate())
        .map(|(idx, entry)| async move {
            let part = if let Some(url) = entry.url {
                // Try to fetch MDX from the URL
                match client.fetch_doc_markdown(&url).await {
                    Ok(mdx) => {
                        // Success: include URL comment and MDX content
                        format!("<!-- Source: {url} -->\n\n{mdx}")
                    }
                    Err(e) => {
                        // Error: include error comment and fallback to original text
                        warn!("Failed to fetch MDX from {url}: {e}");
                        format!("<!-- Error: {e} -->\n\n{}", entry.text)
                    }
                }
            } else {
                // No URL found, use original text content
                entry.text.to_owned()
            };
            (idx, part)
        })
        .buffer_unordered(MAX_CONCURRENT_FETCHES)
        .collect()
        .await;

    // Sort by original index to preserve entry order
    let mut sorted_results = indexed_results;
    sorted_results.sort_by_key(|(idx, _)| *idx);

    // Extract just the parts in order
    let mdx_parts: Vec<String> = sorted_results.into_iter().map(|(_, part)| part).collect();

    // Join with horizontal rules and two newlines
    Ok(mdx_parts.join("\n\n---\n\n"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, reason = "test code")]

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
        // Test that when MDX fetch fails (SSRF rejection for non-bun URL), we get error + fallback
        let result = json!({"content": [{
            "text": "Original text content\nLink: https://evil.com/docs/page",
            "type": "text"
        }]});

        let client = BunDocsClient::new();
        let formatted = format_markdown(&result, &client)
            .await
            .expect("format should succeed");

        // Verify error comment (SSRF rejection) and fallback text
        assert!(
            formatted.contains("<!-- Error:"),
            "Should have error comment when fetch fails"
        );
        assert!(
            formatted.contains("non-bun"),
            "Error should mention SSRF rejection"
        );
        assert!(
            formatted.contains("Original text content"),
            "Should include fallback text"
        );
    }

    #[tokio::test]
    async fn test_format_markdown_ssrf_rejection_http_scheme() {
        // Test that http:// URLs are rejected with fallback
        let result = json!({"content": [{
            "text": "Some docs\nLink: http://bun.com/docs/page",
            "type": "text"
        }]});

        let client = BunDocsClient::new();
        let formatted = format_markdown(&result, &client)
            .await
            .expect("format should succeed");

        assert!(
            formatted.contains("<!-- Error:"),
            "Should have error comment for http URL"
        );
        assert!(
            formatted.contains("non-https"),
            "Error should mention https requirement"
        );
        assert!(
            formatted.contains("Some docs"),
            "Should include fallback text"
        );
    }
}
