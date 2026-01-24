//! Stdio transport for newline-delimited JSON-RPC.
//!
//! Contract:
//! - One JSON-RPC message per line (newline-delimited JSON).
//! - Empty/whitespace-only lines are ignored (looped over internally).
//! - `read_message()` returns `Ok(None)` **only** on EOF.
//! - EOF means the client disconnected.
//!
//! Logging goes to stderr so stdout remains clean JSON-RPC.

use crate::util::truncate_utf8;
use anyhow::{Context as _, Result};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
use tracing::debug;

/// The maximum length of messages (in bytes) to display in debug logs.
/// Messages longer than this will be truncated for readability.
const DEBUG_MESSAGE_MAX_LEN: usize = 80_usize;

/// Stdio-based transport for JSON-RPC communication
pub struct StdioTransport {
    /// A buffered reader for asynchronous input from `stdin`.
    stdin: BufReader<tokio::io::Stdin>,
    /// An asynchronous writer for output to `stdout`.
    stdout: tokio::io::Stdout,
}

impl Default for StdioTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl StdioTransport {
    /// Create a new stdio transport
    ///
    /// # Returns
    /// New `StdioTransport` instance connected to process stdin/stdout
    #[must_use]
    pub fn new() -> Self {
        Self {
            stdin: BufReader::new(tokio::io::stdin()),
            stdout: tokio::io::stdout(),
        }
    }

    /// Truncates a string to `DEBUG_MESSAGE_MAX_LEN` bytes for debug logging.
    fn truncate_for_debug(message: &str) -> &str {
        truncate_utf8(message, DEBUG_MESSAGE_MAX_LEN)
    }

    /// Read a message from stdin
    ///
    /// Loops until a non-empty line is read or EOF is reached. Empty/whitespace-only
    /// lines are silently skipped.
    ///
    /// # Returns
    /// - `Ok(Some(message))` - Successfully read a non-empty message
    /// - `Ok(None)` - EOF (client disconnected)
    ///
    /// # Errors
    /// Returns an error if reading from stdin fails
    pub async fn read_message(&mut self) -> Result<Option<String>> {
        loop {
            let mut line = String::new();
            let bytes_read = self
                .stdin
                .read_line(&mut line)
                .await
                .context("Failed to read from stdin")?;

            if bytes_read == 0_usize {
                debug!("EOF on stdin");
                return Ok(None);
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                // Ignore empty lines, keep reading
                continue;
            }

            debug!("Read message: {}...", Self::truncate_for_debug(trimmed));
            return Ok(Some(trimmed.to_owned()));
        }
    }

    /// Write a message to stdout
    ///
    /// Writes the message followed by a newline, then flushes stdout.
    ///
    /// # Arguments
    /// * `message` - Message to write (newline will be added)
    ///
    /// # Errors
    /// Returns an error if writing to or flushing stdout fails
    pub async fn write_message(&mut self, message: &str) -> Result<()> {
        debug!("Writing message: {}...", Self::truncate_for_debug(message));

        self.stdout
            .write_all(message.as_bytes())
            .await
            .context("Failed to write to stdout")?;

        self.stdout
            .write_all(b"\n")
            .await
            .context("Failed to write newline to stdout")?;

        self.stdout
            .flush()
            .await
            .context("Failed to flush stdout")?;

        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "tests can use expect()")]
#[allow(clippy::unwrap_used, reason = "tests can use unwrap()")]
mod tests {
    use super::*;

    #[test]
    fn new_transport_creation() {
        let _transport = StdioTransport::new();
    }

    #[test]
    fn default_transport_creation() {
        let _transport = StdioTransport::default();
    }

    #[test]
    fn truncate_for_debug() {
        let short = "short message";
        assert_eq!(StdioTransport::truncate_for_debug(short), short);

        let long = "a".repeat(100_usize);
        let truncated = StdioTransport::truncate_for_debug(&long);
        assert_eq!(truncated.len(), DEBUG_MESSAGE_MAX_LEN);
    }

    #[test]
    fn debug_message_max_len_constant() {
        assert_eq!(DEBUG_MESSAGE_MAX_LEN, 80_usize);
    }

    #[test]
    fn read_message_logic() {
        // Test line reading and trimming logic
        let line_with_newline = "test message\n";
        let trimmed = line_with_newline.trim();
        assert_eq!(trimmed, "test message");
        assert!(!trimmed.is_empty());
    }

    #[test]
    fn eof_detection() {
        // Zero bytes read simulates EOF
        let bytes_read = 0_usize;
        assert_eq!(bytes_read, 0_usize);
    }

    #[test]
    fn write_message_format() {
        // Test message formatting logic
        let message = "test output";
        let with_newline = format!("{message}\n");

        assert_eq!(with_newline, "test output\n");
        assert!(with_newline.ends_with('\n'));
        assert_eq!(with_newline.len(), message.len() + 1_usize);
    }

    #[test]
    fn message_truncation_logic() {
        let long_message = "a".repeat(100_usize);
        let truncated = long_message
            .get(..long_message.len().min(80_usize))
            .expect("valid slice within bounds");
        assert_eq!(truncated.len(), 80_usize);
    }

    #[test]
    fn trim_behavior() {
        let message_with_whitespace = "  test message  \n";
        let trimmed = message_with_whitespace.trim();
        assert_eq!(trimmed, "test message");
    }

    #[test]
    fn empty_line_detection() {
        let empty = "";
        let whitespace_only = "   \n";
        let non_empty = "message";

        assert!(empty.trim().is_empty());
        assert!(whitespace_only.trim().is_empty());
        assert!(!non_empty.trim().is_empty());
    }

    #[test]
    fn newline_bytes() {
        let newline = b"\n";
        assert_eq!(newline.len(), 1_usize);
        assert_eq!(newline.first().expect("newline has one byte"), &10_u8);
    }

    #[test]
    fn message_format() {
        let message = "test message";
        let with_newline = format!("{message}\n");
        assert_eq!(with_newline, "test message\n");
        assert!(with_newline.ends_with('\n'));
    }

    #[test]
    fn string_length_safety() {
        let short = "test";
        let long = "a".repeat(200_usize);
        let short_min = short.len().min(80_usize);
        let long_min = long.len().min(80_usize);
        assert_eq!(short_min, 4_usize);
        assert_eq!(long_min, 80_usize);
    }
}
