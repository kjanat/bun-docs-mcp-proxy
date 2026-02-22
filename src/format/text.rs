//! Plain text output formatter.

use anyhow::Result;

use super::extract_content_texts;

/// Formats a search result as a plain text string.
///
/// It extracts the text content from the result and joins it with newlines.
/// If no text content is found, it falls back to a pretty-printed JSON representation.
///
/// # Arguments
/// * `result` - A reference to the `serde_json::Value` to format.
///
/// # Returns
/// A `Result` containing the formatted plain text string.
///
/// # Errors
///
/// Returns an error if JSON serialization fails during fallback formatting.
pub fn format_text(result: &serde_json::Value) -> Result<String> {
    let texts = extract_content_texts(result);

    if texts.is_empty() {
        Ok(serde_json::to_string_pretty(result)?)
    } else {
        Ok(texts.join("\n\n"))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test code")]

    use serde_json::json;

    use super::*;

    #[test]
    fn test_format_text() {
        let result = json!({"content": [{"text": "test content", "type": "text"}]});
        let formatted = format_text(&result).unwrap();
        assert!(formatted.contains("test content"));
        assert!(!formatted.contains("\"content\""));
    }

    #[test]
    fn test_format_text_no_content() {
        let result = json!({"other": "data"});
        let formatted = format_text(&result).unwrap();
        assert!(formatted.contains("\"other\""));
        assert!(formatted.contains("\"data\""));
    }

    #[test]
    fn test_format_text_empty_content_array() {
        let result = json!({"content": []});
        let formatted = format_text(&result).unwrap();
        // Empty content array falls back to JSON
        assert!(formatted.contains("\"content\": []"));
    }

    #[test]
    fn test_format_text_multiple_items() {
        let result = json!({"content": [
            {"text": "first item", "type": "text"},
            {"text": "second item", "type": "text"}
        ]});
        let formatted = format_text(&result).unwrap();
        assert!(formatted.contains("first item"));
        assert!(formatted.contains("second item"));
    }

    #[test]
    fn test_format_text_with_null_content() {
        let result = json!({"content": null, "other": "data"});
        let formatted = format_text(&result).unwrap();
        assert!(formatted.contains("\"content\": null"));
    }
}
