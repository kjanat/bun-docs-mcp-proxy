//! Shared utility functions.

/// Truncates a string to a maximum byte length, ensuring UTF-8 boundary safety.
///
/// If the string's byte length is already <= `max_len`, returns the original slice.
/// Otherwise, finds the last valid UTF-8 character boundary at or before `max_len`.
///
/// # Arguments
/// * `text` - The string slice to truncate.
/// * `max_len` - The maximum desired length in bytes.
///
/// # Returns
/// A string slice that is a valid UTF-8 truncation of the input.
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
    text.get(..last_valid).unwrap_or("")
}

/// Truncates a string for logging, adding ellipsis if truncated.
///
/// # Arguments
/// * `text` - The string to truncate.
/// * `max_len` - Maximum length before truncation.
///
/// # Returns
/// The original string if shorter than `max_len`, otherwise truncated with "..." suffix.
#[must_use]
pub fn truncate_for_log(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        text.to_owned()
    } else {
        let truncated = truncate_utf8(text, max_len);
        format!("{truncated}...")
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests can use unwrap()")]
mod tests {
    use super::*;

    #[test]
    fn truncate_utf8_short_string() {
        let short = "hello";
        assert_eq!(truncate_utf8(short, 10_usize), short);
    }

    #[test]
    fn truncate_utf8_long_string() {
        let long = "a".repeat(100_usize);
        let truncated = truncate_utf8(&long, 50_usize);
        assert!(truncated.len() <= 50_usize);
        assert!(!truncated.is_empty());
    }

    #[test]
    fn truncate_utf8_unicode_boundary() {
        // "hello " (7 bytes) + 2 CJK chars (3 bytes each)
        let unicode = "hello \u{4e16}\u{754c}"; // "hello 世界"
        let truncated = truncate_utf8(unicode, 8_usize);
        // Should truncate at UTF-8 boundary, not mid-character
        assert!(truncated.len() <= 8_usize);
        assert!(truncated.is_char_boundary(truncated.len()));
        assert_eq!(truncated, "hello "); // Can't fit the first CJK char
    }

    #[test]
    fn truncate_utf8_exact_boundary() {
        let text = "abc";
        assert_eq!(truncate_utf8(text, 3_usize), "abc");
    }

    #[test]
    fn truncate_for_log_short() {
        let short = "test";
        assert_eq!(truncate_for_log(short, 10_usize), "test");
    }

    #[test]
    fn truncate_for_log_long() {
        let long = "a".repeat(100_usize);
        let result = truncate_for_log(&long, 50_usize);
        assert!(result.ends_with("..."));
        assert!(result.len() <= 53_usize); // 50 + "..."
    }
}
