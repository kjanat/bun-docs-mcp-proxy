//! Bun Docs MCP Proxy - Protocol adapter for Bun documentation.
//!
//! This crate provides an MCP (Model Context Protocol) proxy that bridges
//! stdio-based MCP clients with the HTTP/SSE-based Bun documentation server.
//!
//! # Modules
//!
//! - [`app`] - Application state and MCP server loop
//! - [`format`] - Output formatters (JSON, text, markdown)
//! - [`io`] - I/O transport layer
//! - [`mcp`] - MCP protocol types and constants
//! - [`upstream`] - Upstream API clients
//! - [`util`] - Utility functions

pub mod app;
pub mod format;
pub mod io;
pub mod mcp;
pub mod upstream;
pub mod util;

// Convenience re-exports
pub use app::run_mcp_server;
pub use io::{StdioTransport, Transport};
pub use mcp::{
    JsonRpcEnvelope, JsonRpcError, JsonRpcErrorObject, JsonRpcRequest, JsonRpcResponse,
    LINK_MARKER, MCP_PROTOCOL_VERSION, Method, SERVER_NAME, content_type, error_code,
};
pub use upstream::{BunDocsClient, BunDocsClientBuilder, BunDocsClientConfig, UpstreamResponse};
