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
use tokio::io::{AsyncBufReadExt as _, AsyncRead, AsyncWrite, AsyncWriteExt as _, BufReader};
use tracing::debug;

/// The maximum length of messages (in bytes) to display in debug logs.
/// Messages longer than this will be truncated for readability.
const DEBUG_MESSAGE_MAX_LEN: usize = 80_usize;

/// Generic transport for JSON-RPC communication over async streams.
///
/// This struct is parameterized over:
/// - `R`: An async reader implementing `AsyncRead + Unpin`
/// - `W`: An async writer implementing `AsyncWrite + Unpin`
///
/// The reader is internally wrapped in a `BufReader` for efficient line-based reading.
pub struct Transport<R, W> {
    /// A buffered reader for asynchronous input.
    reader: BufReader<R>,
    /// An asynchronous writer for output.
    writer: W,
}

impl<R, W> Transport<R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    /// Create a new transport from raw reader and writer.
    ///
    /// The reader will be wrapped in a `BufReader` for efficient line-based reading.
    ///
    /// # Arguments
    /// * `reader` - Async reader (will be wrapped in `BufReader`)
    /// * `writer` - Async writer
    ///
    /// # Returns
    /// New `Transport` instance
    #[must_use]
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            reader: BufReader::new(reader),
            writer,
        }
    }

    /// Truncates a string to `DEBUG_MESSAGE_MAX_LEN` bytes for debug logging.
    fn truncate_for_debug(message: &str) -> &str {
        truncate_utf8(message, DEBUG_MESSAGE_MAX_LEN)
    }

    /// Read a message from the input stream.
    ///
    /// Loops until a non-empty line is read or EOF is reached. Empty/whitespace-only
    /// lines are silently skipped.
    ///
    /// # Returns
    /// - `Ok(Some(message))` - Successfully read a non-empty message
    /// - `Ok(None)` - EOF (client disconnected)
    ///
    /// # Errors
    /// Returns an error if reading from the input stream fails
    pub async fn read_message(&mut self) -> Result<Option<String>> {
        loop {
            let mut line = String::new();
            let bytes_read = self
                .reader
                .read_line(&mut line)
                .await
                .context("Failed to read from input")?;

            if bytes_read == 0_usize {
                debug!("EOF on input");
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

    /// Write a message to the output stream.
    ///
    /// Writes the message followed by a newline, then flushes the output.
    ///
    /// # Arguments
    /// * `message` - Message to write (newline will be added)
    ///
    /// # Errors
    /// Returns an error if writing to or flushing the output stream fails
    pub async fn write_message(&mut self, message: &str) -> Result<()> {
        debug!("Writing message: {}...", Self::truncate_for_debug(message));

        self.writer
            .write_all(message.as_bytes())
            .await
            .context("Failed to write to output")?;

        self.writer
            .write_all(b"\n")
            .await
            .context("Failed to write newline to output")?;

        self.writer
            .flush()
            .await
            .context("Failed to flush output")?;

        Ok(())
    }
}

/// Convenience type alias for stdio-based transport.
pub type StdioTransport = Transport<tokio::io::Stdin, tokio::io::Stdout>;

impl StdioTransport {
    /// Create a new stdio transport connected to process stdin/stdout.
    ///
    /// # Returns
    /// New `StdioTransport` instance connected to process stdin/stdout
    #[must_use]
    #[allow(
        clippy::use_self,
        reason = "Self is a type alias, Transport works here"
    )]
    pub fn stdio() -> Self {
        Transport::new(tokio::io::stdin(), tokio::io::stdout())
    }
}

impl Default for StdioTransport {
    fn default() -> Self {
        Self::stdio()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "tests can use expect()")]
#[allow(clippy::unwrap_used, reason = "tests can use unwrap()")]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Helper type for in-memory transport testing.
    type TestTransport = Transport<Cursor<Vec<u8>>, Vec<u8>>;

    /// Create a test transport with predefined input.
    fn test_transport(input: &str) -> TestTransport {
        Transport::new(Cursor::new(input.as_bytes().to_vec()), Vec::new())
    }

    #[tokio::test]
    async fn read_single_message() {
        let mut transport = test_transport("hello world\n");
        let msg = transport.read_message().await.unwrap();
        assert_eq!(msg, Some("hello world".to_owned()));
    }

    #[tokio::test]
    async fn read_multiple_messages() {
        let mut transport = test_transport("first\nsecond\nthird\n");

        assert_eq!(
            transport.read_message().await.unwrap(),
            Some("first".to_owned())
        );
        assert_eq!(
            transport.read_message().await.unwrap(),
            Some("second".to_owned())
        );
        assert_eq!(
            transport.read_message().await.unwrap(),
            Some("third".to_owned())
        );
        assert_eq!(transport.read_message().await.unwrap(), None);
    }

    #[tokio::test]
    async fn read_skips_empty_lines() {
        let mut transport = test_transport("\n\n\nhello\n\n\n");
        let msg = transport.read_message().await.unwrap();
        assert_eq!(msg, Some("hello".to_owned()));
    }

    #[tokio::test]
    async fn read_skips_whitespace_only_lines() {
        let mut transport = test_transport("   \n\t\t\n  \t  \nmessage\n");
        let msg = transport.read_message().await.unwrap();
        assert_eq!(msg, Some("message".to_owned()));
    }

    #[tokio::test]
    async fn read_trims_whitespace() {
        let mut transport = test_transport("  trimmed message  \n");
        let msg = transport.read_message().await.unwrap();
        assert_eq!(msg, Some("trimmed message".to_owned()));
    }

    #[tokio::test]
    async fn read_eof_returns_none() {
        let mut transport = test_transport("");
        let msg = transport.read_message().await.unwrap();
        assert_eq!(msg, None);
    }

    #[tokio::test]
    async fn read_eof_after_empty_lines() {
        let mut transport = test_transport("\n\n\n");
        let msg = transport.read_message().await.unwrap();
        assert_eq!(msg, None);
    }

    #[tokio::test]
    async fn write_appends_newline() {
        let mut transport = test_transport("");
        transport.write_message("hello").await.unwrap();
        assert_eq!(transport.writer, b"hello\n");
    }

    #[tokio::test]
    async fn write_multiple_messages() {
        let mut transport = test_transport("");
        transport.write_message("first").await.unwrap();
        transport.write_message("second").await.unwrap();
        assert_eq!(transport.writer, b"first\nsecond\n");
    }

    #[tokio::test]
    async fn write_empty_message() {
        let mut transport = test_transport("");
        transport.write_message("").await.unwrap();
        assert_eq!(transport.writer, b"\n");
    }

    #[tokio::test]
    async fn roundtrip_json_message() {
        let json = r#"{"jsonrpc":"2.0","id":1,"method":"test"}"#;
        let mut transport = test_transport(&format!("{json}\n"));

        let read = transport.read_message().await.unwrap().unwrap();
        assert_eq!(read, json);

        transport.write_message(&read).await.unwrap();
        assert_eq!(
            String::from_utf8_lossy(&transport.writer),
            format!("{json}\n")
        );
    }

    #[test]
    fn stdio_transport_creation() {
        // Just verify the type alias compiles and stdio() works
        let _transport: StdioTransport = StdioTransport::stdio();
    }

    #[test]
    fn default_transport_creation() {
        let _transport = StdioTransport::default();
    }

    #[test]
    fn truncate_for_debug_short() {
        let short = "short message";
        assert_eq!(
            Transport::<Cursor<Vec<u8>>, Vec<u8>>::truncate_for_debug(short),
            short
        );
    }

    #[test]
    fn truncate_for_debug_long() {
        let long = "a".repeat(100_usize);
        let truncated = Transport::<Cursor<Vec<u8>>, Vec<u8>>::truncate_for_debug(&long);
        assert_eq!(truncated.len(), DEBUG_MESSAGE_MAX_LEN);
    }

    #[test]
    fn debug_message_max_len_constant() {
        assert_eq!(DEBUG_MESSAGE_MAX_LEN, 80_usize);
    }
}
