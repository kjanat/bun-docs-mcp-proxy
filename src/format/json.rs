//! JSON output formatter.

use anyhow::Result;

/// Formats a search result as a pretty-printed JSON string.
///
/// # Arguments
/// * `result` - A reference to the `serde_json::Value` to format.
///
/// # Returns
/// A `Result` containing the formatted JSON string, or an error if serialization fails.
///
/// # Errors
///
/// Returns an error if JSON serialization fails.
pub fn format_json(result: &serde_json::Value) -> Result<String> {
    Ok(serde_json::to_string_pretty(result)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_format_json() {
        let result = json!({"content": [{"text": "test", "type": "text"}]});
        let formatted = format_json(&result).unwrap();
        assert!(formatted.contains("\"content\""));
        assert!(formatted.contains("\"text\": \"test\""));
    }

    #[test]
    fn test_format_json_empty() {
        let result = json!({});
        let formatted = format_json(&result).unwrap();
        assert_eq!(formatted, "{}");
    }
}
