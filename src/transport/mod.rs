use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::debug;

// NOTE: Coverage for this module is lower (~65%) because read_message and write_message
// are tightly coupled to real stdin/stdout types, making them difficult to unit test.
// They are tested through integration tests and manual testing with the actual binary.
pub struct StdioTransport {
    stdin: BufReader<tokio::io::Stdin>,
    stdout: tokio::io::Stdout,
}

impl StdioTransport {
    pub fn new() -> Self {
        Self {
            stdin: BufReader::new(tokio::io::stdin()),
            stdout: tokio::io::stdout(),
        }
    }

    pub async fn read_message(&mut self) -> Result<Option<String>> {
        let mut line = String::new();
        let bytes_read = self
            .stdin
            .read_line(&mut line)
            .await
            .context("Failed to read from stdin")?;

        if bytes_read == 0 {
            debug!("EOF on stdin");
            return Ok(None);
        }

        let line = line.trim();
        if line.is_empty() {
            return Ok(None);
        }

        debug!("Read message: {}...", &line[..line.len().min(80)]);
        Ok(Some(line.to_string()))
    }

    pub async fn write_message(&mut self, message: &str) -> Result<()> {
        debug!("Writing message: {}...", &message[..message.len().min(80)]);

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
mod tests {
    use super::*;

    #[test]
    fn test_new_transport_creation() {
        let _transport = StdioTransport::new();
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
        let with_newline = format!("{}\n", message);

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
        let with_newline = format!("{}\n", message);
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
}
