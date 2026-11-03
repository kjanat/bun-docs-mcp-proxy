//! MCP protocol types, constants, and helpers.
//!
//! This module centralizes MCP protocol definitions including JSON-RPC types,
//! method names, error codes, and other protocol constants.

pub mod protocol;

// Re-export protocol types for convenience
use core::{fmt, str::FromStr};

#[allow(unused_imports, reason = "re-exports for public API consistency")]
pub use protocol::{
    JsonRpcEnvelope, JsonRpcError, JsonRpcErrorObject, JsonRpcRequest, JsonRpcResponse,
};

// ============================================================================
// JSON-RPC Method Names
// ============================================================================

/// MCP protocol method names.
///
/// Used for type-safe method dispatch in the main request handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// Discover supported protocol versions and server capabilities
    Discover,
    /// List available tools
    ToolsList,
    /// Call a tool
    ToolsCall,
}

impl Method {
    /// Returns the string representation of the method.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Discover => "server/discover",
            Self::ToolsList => "tools/list",
            Self::ToolsCall => "tools/call",
        }
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Method {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "server/discover" => Ok(Self::Discover),
            "tools/list" => Ok(Self::ToolsList),
            "tools/call" => Ok(Self::ToolsCall),
            _ => Err(()),
        }
    }
}

// ============================================================================
// Content Types
// ============================================================================

/// HTTP content type constants.
pub mod content_type {
    /// JSON content type
    pub const JSON: &str = "application/json";
    /// Server-Sent Events content type
    pub const SSE: &str = "text/event-stream";
    /// Markdown content type
    pub const MARKDOWN: &str = "text/markdown";
}

// ============================================================================
// Protocol Constants
// ============================================================================

/// Current published MCP protocol revision implemented by this server.
pub const MCP_PROTOCOL_VERSION: &str = "2026-07-28";

/// Validates the required per-request metadata and MCP request identifier.
pub(crate) fn request_error(
    request: &JsonRpcRequest,
    request_id: serde_json::Value,
) -> Option<JsonRpcResponse> {
    if !request_id.is_string() && !request_id.is_i64() && !request_id.is_u64() {
        return Some(JsonRpcResponse::error(
            serde_json::Value::Null,
            error_code::INVALID_REQUEST,
            "MCP request id must be a string or integer".to_owned(),
        ));
    }
    // Give clients using the removed handshake an actionable diagnostic.
    if request.method == "initialize" {
        return Some(JsonRpcResponse::error(
            request_id,
            error_code::METHOD_NOT_FOUND,
            format!(
                "initialize is not supported; use MCP {MCP_PROTOCOL_VERSION} per-request metadata and server/discover"
            ),
        ));
    }
    let meta = request
        .params
        .as_ref()
        .and_then(|params| params.get("_meta"));
    let Some(version) = meta
        .and_then(|metadata| metadata.get("io.modelcontextprotocol/protocolVersion"))
        .and_then(serde_json::Value::as_str)
    else {
        return Some(JsonRpcResponse::error(
            request_id,
            error_code::INVALID_PARAMS,
            "params._meta must include a string io.modelcontextprotocol/protocolVersion".to_owned(),
        ));
    };
    if version != MCP_PROTOCOL_VERSION {
        return Some(JsonRpcResponse::error_with_data(
            request_id,
            error_code::UNSUPPORTED_PROTOCOL_VERSION,
            "Unsupported protocol version".to_owned(),
            serde_json::json!({ "supported": [MCP_PROTOCOL_VERSION], "requested": version }),
        ));
    }
    if !meta
        .and_then(|metadata| metadata.get("io.modelcontextprotocol/clientCapabilities"))
        .is_some_and(serde_json::Value::is_object)
    {
        return Some(JsonRpcResponse::error(
            request_id,
            error_code::INVALID_PARAMS,
            "params._meta must include an object io.modelcontextprotocol/clientCapabilities"
                .to_owned(),
        ));
    }
    if request.method == "tools/list"
        && request
            .params
            .as_ref()
            .and_then(|params| params.get("cursor"))
            .is_some()
    {
        return Some(JsonRpcResponse::error(
            request_id,
            error_code::INVALID_PARAMS,
            "Invalid cursor: this server returns its complete tool list in one page".to_owned(),
        ));
    }
    if request.params.as_ref().is_some_and(|params| {
        params.get("inputResponses").is_some() || params.get("requestState").is_some()
    }) {
        return Some(JsonRpcResponse::error(
            request_id,
            error_code::INVALID_PARAMS,
            "This server has no input requests to resume".to_owned(),
        ));
    }
    None
}

/// Server name reported by discovery.
pub const SERVER_NAME: &str = "bun-docs";

/// Line marker prefix for documentation links in search results.
pub const LINK_MARKER: &str = "Link: ";

// ============================================================================
// JSON-RPC Error Codes
// ============================================================================

/// Standard JSON-RPC 2.0 error codes.
pub mod error_code {
    /// Parse error (invalid JSON) - `-32700`
    pub const PARSE_ERROR: i32 = -32700;
    /// Invalid request (malformed JSON-RPC) - `-32600`
    #[allow(dead_code, reason = "included for protocol completeness")]
    pub const INVALID_REQUEST: i32 = -32600;
    /// Method not found - `-32601`
    pub const METHOD_NOT_FOUND: i32 = -32601;
    /// Invalid params - `-32602`
    pub const INVALID_PARAMS: i32 = -32602;
    /// Internal error - `-32603`
    pub const INTERNAL_ERROR: i32 = -32603;
    /// Requested MCP protocol revision is unsupported.
    pub const UNSUPPORTED_PROTOCOL_VERSION: i32 = -32022;
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests can use unwrap()")]
mod tests {
    use super::*;

    #[test]
    fn method_as_str() {
        assert_eq!(Method::Discover.as_str(), "server/discover");
        assert_eq!(Method::ToolsList.as_str(), "tools/list");
        assert_eq!(Method::ToolsCall.as_str(), "tools/call");
    }

    #[test]
    fn method_display() {
        assert_eq!(format!("{}", Method::Discover), "server/discover");
        assert_eq!(format!("{}", Method::ToolsCall), "tools/call");
    }

    #[test]
    fn method_from_str_valid() {
        assert_eq!("server/discover".parse::<Method>(), Ok(Method::Discover));
        assert_eq!("tools/list".parse::<Method>(), Ok(Method::ToolsList));
        assert_eq!("tools/call".parse::<Method>(), Ok(Method::ToolsCall));
    }

    #[test]
    fn method_from_str_invalid() {
        let _: () = "unknown".parse::<Method>().unwrap_err();
        let _: () = "".parse::<Method>().unwrap_err();
        let _: () = "Server/discover".parse::<Method>().unwrap_err(); // case sensitive
        let _: () = "initialize".parse::<Method>().unwrap_err();
        let _: () = "notifications/initialized".parse::<Method>().unwrap_err();
        let _: () = "resources/list".parse::<Method>().unwrap_err(); // removed
        let _: () = "resources/read".parse::<Method>().unwrap_err(); // removed
    }

    #[test]
    fn content_type_constants() {
        assert_eq!(content_type::JSON, "application/json");
        assert_eq!(content_type::SSE, "text/event-stream");
        assert_eq!(content_type::MARKDOWN, "text/markdown");
    }

    #[test]
    fn protocol_constants() {
        assert_eq!(MCP_PROTOCOL_VERSION, "2026-07-28");
        assert_eq!(SERVER_NAME, "bun-docs");
        assert_eq!(LINK_MARKER, "Link: ");
    }

    #[test]
    fn error_code_constants() {
        assert_eq!(error_code::PARSE_ERROR, -32_700_i32);
        assert_eq!(error_code::INVALID_REQUEST, -32_600_i32);
        assert_eq!(error_code::METHOD_NOT_FOUND, -32_601_i32);
        assert_eq!(error_code::INVALID_PARAMS, -32_602_i32);
        assert_eq!(error_code::INTERNAL_ERROR, -32_603_i32);
        assert_eq!(error_code::UNSUPPORTED_PROTOCOL_VERSION, -32_022_i32);
    }
}
