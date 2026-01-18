//! Shared utility functions
//!
//! This module provides common helper functions used across multiple modules,
//! reducing code duplication and centralizing utility logic.

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
///
/// # Examples
/// ```
/// # use bun_docs_mcp_proxy::utils::truncate_utf8;
/// assert_eq!(truncate_utf8("hello", 10), "hello");
/// assert_eq!(truncate_utf8("hello world", 5), "hello");
/// // Unicode safety: won't split multi-byte chars
/// assert_eq!(truncate_utf8("hello", 100), "hello");
/// ```
#[must_use]
pub fn truncate_utf8(text: &str, max_len: usize) -> &str {
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

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests can use unwrap()")]
mod tests {
    use super::*;

    #[test]
    fn truncate_short_string() {
        let short = "hello";
        assert_eq!(truncate_utf8(short, 10_usize), short);
    }

    #[test]
    fn truncate_long_string() {
        let long = "a".repeat(100_usize);
        let truncated = truncate_utf8(&long, 50_usize);
        assert_eq!(truncated.len(), 50_usize);
    }

    #[test]
    fn truncate_exact_length() {
        let text = "hello";
        assert_eq!(truncate_utf8(text, 5_usize), "hello");
    }

    #[test]
    fn truncate_unicode_safety() {
        // Multi-byte UTF-8: each CJK char is 3 bytes
        let unicode = "\u{4e16}\u{754c}"; // 6 bytes total
        let truncated = truncate_utf8(unicode, 4_usize);
        // Should truncate to first char only (3 bytes), not split
        assert_eq!(truncated.len(), 3_usize);
        assert!(truncated.is_char_boundary(truncated.len()));
    }

    #[test]
    fn truncate_empty_string() {
        assert_eq!(truncate_utf8("", 10_usize), "");
    }

    #[test]
    fn truncate_zero_max() {
        assert_eq!(truncate_utf8("hello", 0_usize), "");
    }
}
