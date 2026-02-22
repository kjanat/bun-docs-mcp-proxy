//! Output formatters for search results.

mod json;
mod markdown;
mod text;

pub use json::format_json;
pub use markdown::format_markdown;
pub use text::format_text;

use crate::mcp::LINK_MARKER;

/// Represents a single documentation entry, which may have a URL for fetching the
/// full content and always has fallback text from the initial search result.
#[derive(Debug, Clone)]
pub struct DocEntry<'text> {
    /// An optional URL to the full documentation page.
    pub url: Option<String>,
    /// The fallback text content, extracted from the search result.
    pub text: &'text str,
}

/// Extracts all text content from a search result's `content` array.
///
/// The search result is expected to be a JSON object with a `content` field,
/// which is an array of objects, each with a `text` field.
///
/// # Arguments
/// * `result` - A reference to the `serde_json::Value` representing the search result.
///
/// # Returns
/// A `Vec<&str>` containing all the extracted text slices. Returns an empty vector
/// if `content` is missing or not an array.
#[must_use]
pub fn extract_content_texts(result: &serde_json::Value) -> Vec<&str> {
    result
        .get("content")
        .and_then(|c| c.as_array())
        .map(|content| {
            content
                .iter()
                .filter_map(|item| item.get("text").and_then(|t| t.as_str()))
                .collect()
        })
        .unwrap_or_default()
}

/// Parses search result content to create a vector of `DocEntry` structs.
///
/// This function iterates through the text content of a search result, looking for
/// `Link:` annotations to extract URLs. It creates a `DocEntry` for each piece of
/// content, containing the URL (if found) and the original text as a fallback.
///
/// # Arguments
/// * `result` - A reference to the `serde_json::Value` representing the search result.
///
/// # Returns
/// A `Vec<DocEntry>` containing the parsed documentation entries.
#[must_use]
pub fn extract_doc_entries(result: &serde_json::Value) -> Vec<DocEntry<'_>> {
    let texts = extract_content_texts(result);

    texts
        .into_iter()
        .map(|text| {
            // Parse "Link: <URL>" pattern
            let url = text.lines().find_map(|line| {
                let trimmed = line.trim();
                trimmed
                    .strip_prefix(LINK_MARKER)
                    .map(|url_part| url_part.trim().to_owned())
            });

            DocEntry { url, text }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, reason = "test code")]

    use serde_json::json;

    use super::*;

    #[test]
    fn test_extract_doc_entries_with_url() {
        let result = json!({"content": [{
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
        let result = json!({"content": [{
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
        let result = json!({"content": []});
        let entries = extract_doc_entries(&result);
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn test_extract_doc_entries_multiple_with_mixed_urls() {
        let result = json!({"content": [
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
        let result = json!({"content": [
            {"text": "first", "type": "text"},
            {"text": "second", "type": "text"}
        ]});
        let texts = extract_content_texts(&result);
        assert_eq!(texts, vec!["first", "second"]);
    }

    #[test]
    fn test_extract_content_texts_empty() {
        let result = json!({});
        let texts = extract_content_texts(&result);
        assert!(texts.is_empty());
    }

    #[test]
    fn test_extract_content_texts_null_content() {
        let result = json!({"content": null});
        let texts = extract_content_texts(&result);
        assert!(texts.is_empty());
    }

    #[test]
    fn test_extract_content_texts_non_array_content() {
        let result = json!({"content": "not an array"});
        let texts = extract_content_texts(&result);
        assert!(texts.is_empty());
    }

    #[test]
    fn test_extract_content_texts_missing_text_field() {
        let result = json!({"content": [
            {"type": "text"},  // missing text field
            {"text": "valid", "type": "text"}
        ]});
        let texts = extract_content_texts(&result);
        assert_eq!(texts, vec!["valid"]);
    }

    #[test]
    fn test_extract_content_texts_empty_string() {
        let result = json!({"content": [
            {"text": "", "type": "text"},
            {"text": "valid", "type": "text"}
        ]});
        let texts = extract_content_texts(&result);
        assert_eq!(texts, vec!["", "valid"]);
    }

    #[test]
    fn test_extract_content_texts_non_string_text() {
        let result = json!({"content": [
            {"text": 123_i32, "type": "text"},  // text is number
            {"text": "valid", "type": "text"}
        ]});
        let texts = extract_content_texts(&result);
        assert_eq!(texts, vec!["valid"]);
    }
}
