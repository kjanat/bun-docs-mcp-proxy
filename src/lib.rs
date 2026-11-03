//! Local Bun documentation search exposed over MCP and a command-line interface.
//!
//! This crate downloads Bun's static documentation corpus, indexes it locally,
//! and serves searches to stdio-based MCP clients without an upstream MCP server.
//!
//! # Modules
//!
//! - [`app`] - Application state and MCP server loop
//! - [`io`] - I/O transport layer
//! - [`mcp`] - MCP protocol types and constants
//! - [`util`] - Utility functions

pub mod app;
pub mod docs;
pub mod io;
pub mod mcp;
pub mod util;

// Convenience re-exports
pub use app::run_mcp_server;
pub use io::{StdioTransport, Transport};
pub use mcp::{
    JsonRpcEnvelope, JsonRpcError, JsonRpcErrorObject, JsonRpcRequest, JsonRpcResponse,
    LINK_MARKER, MCP_PROTOCOL_VERSION, Method, SERVER_NAME, content_type, error_code,
};
