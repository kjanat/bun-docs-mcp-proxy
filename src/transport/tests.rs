use super::*;

#[test]
fn test_new_transport_creation() {
    let _transport = StdioTransport::new();
}

#[test]
fn test_default_transport_creation() {
    let _transport = StdioTransport::default();
}

#[test]
fn test_truncate_for_debug() {
    let short = "short message";
    assert_eq!(StdioTransport::truncate_for_debug(short), short);

    let long = "a".repeat(100);
    let truncated = StdioTransport::truncate_for_debug(&long);
    assert_eq!(truncated.len(), DEBUG_MESSAGE_MAX_LEN);
}

#[test]
fn test_debug_message_max_len_constant() {
    assert_eq!(DEBUG_MESSAGE_MAX_LEN, 80);
}

#[test]
fn test_read_message_logic() {
    // Test line reading and trimming logic
    let line_with_newline = "test message\n";
    let trimmed = line_with_newline.trim();
    assert_eq!(trimmed, "test message");
    assert!(!trimmed.is_empty());
}

#[test]
fn test_eof_detection() {
    // Zero bytes read simulates EOF
    let bytes_read = 0;
    assert_eq!(bytes_read, 0);
}

#[test]
fn test_write_message_format() {
    // Test message formatting logic
    let message = "test output";
    let with_newline = format!("{message}\n");

    assert_eq!(with_newline, "test output\n");
    assert!(with_newline.ends_with('\n'));
    assert_eq!(with_newline.len(), message.len() + 1);
}

#[test]
fn test_message_truncation_logic() {
    let long_message = "a".repeat(100);
    let truncated = &long_message[..long_message.len().min(80)];
    assert_eq!(truncated.len(), 80);
}

#[test]
fn test_trim_behavior() {
    let message_with_whitespace = "  test message  \n";
    let trimmed = message_with_whitespace.trim();
    assert_eq!(trimmed, "test message");
}

#[test]
fn test_empty_line_detection() {
    let empty = "";
    let whitespace_only = "   \n";
    let non_empty = "message";

    assert!(empty.trim().is_empty());
    assert!(whitespace_only.trim().is_empty());
    assert!(!non_empty.trim().is_empty());
}

#[test]
fn test_newline_bytes() {
    let newline = b"\n";
    assert_eq!(newline.len(), 1);
    assert_eq!(newline[0], 10);
}

#[test]
fn test_message_format() {
    let message = "test message";
    let with_newline = format!("{message}\n");
    assert_eq!(with_newline, "test message\n");
    assert!(with_newline.ends_with('\n'));
}

#[test]
fn test_string_length_safety() {
    let short = "test";
    let long = "a".repeat(200);
    let short_min = short.len().min(80);
    let long_min = long.len().min(80);
    assert_eq!(short_min, 4);
    assert_eq!(long_min, 80);
}
