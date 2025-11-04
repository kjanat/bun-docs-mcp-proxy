//! Stdio transport layer for JSON-RPC communication
//!
//! This module provides an async stdio transport for reading JSON-RPC requests from stdin
//! and writing JSON-RPC responses to stdout. It's designed for use with MCP (Model Context
//! Protocol) clients that communicate over stdio, such as the Zed editor.
//!
//! ## Message Format
//!
//! - Messages are newline-delimited JSON (one JSON-RPC message per line)
//! - Empty lines are ignored
//! - EOF on stdin signals connection closure
//!
//! ## Logging
//!
//! All logging goes to stderr (not stdout) to avoid interfering with JSON-RPC messages.
//! Long messages are truncated to [`DEBUG_MESSAGE_MAX_LEN`] characters in debug logs.
//!
//! ## Test Coverage Note
//!
//! Coverage for this module is lower (~56%) because `read_message` and `write_message`
//! are tightly coupled to real stdin/stdout types, making them difficult to unit test.
//! They are tested through integration tests and manual testing with the actual binary.

use anyhow::{Context as _, Result};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
use tracing::debug;

// Maximum length of messages to show in debug logs
const DEBUG_MESSAGE_MAX_LEN: usize = 80;
pub struct StdioTransport {
    stdin: BufReader<tokio::io::Stdin>,
    stdout: tokio::io::Stdout,
}

impl Default for StdioTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl StdioTransport {
    pub fn new() -> Self {
        Self {
            stdin: BufReader::new(tokio::io::stdin()),
            stdout: tokio::io::stdout(),
        }
    }

    /// Truncate message for debug logging, preserving UTF-8 boundaries
    fn truncate_for_debug(message: &str) -> &str {
        if message.len() <= DEBUG_MESSAGE_MAX_LEN {
            return message;
        }
        // Find the last char whose end position is at or before max length
        let mut last_valid = 0;
        for (idx, ch) in message.char_indices() {
            let end_pos = idx + ch.len_utf8();
            if end_pos > DEBUG_MESSAGE_MAX_LEN {
                break;
            }
            last_valid = end_pos;
        }
        &message[..last_valid]
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

        debug!("Read message: {}...", Self::truncate_for_debug(line));
        return Ok(Some(line.to_owned()));
    }

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
mod tests;
