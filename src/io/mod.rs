//! I/O transport layer for MCP communication.

pub mod transport;

// Re-export transport types (used by app module)
#[allow(unused_imports, reason = "re-exports for public API consistency")]
pub use transport::{StdioTransport, Transport};
