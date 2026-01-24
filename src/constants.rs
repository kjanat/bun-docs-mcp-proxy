//! Constants and enums for the Bun Docs MCP Proxy.
//!
//! This module centralizes magic strings and protocol constants to reduce
//! typo risk and improve maintainability.

use core::fmt;
use core::str::FromStr;

// ============================================================================
// JSON-RPC Method Names
// ============================================================================

/// MCP protocol method names.
///
/// Used for type-safe method dispatch in the main request handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// Initialize the MCP connection
    Initialize,
    /// List available tools
    ToolsList,
    /// Call a tool
    ToolsCall,
    /// List available resources
    ResourcesList,
    /// Read a resource
    ResourcesRead,
    /// Notification that client is initialized (no response expected)
    NotificationsInitialized,
}

impl Method {
    /// Returns the string representation of the method.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Initialize => "initialize",
            Self::ToolsList => "tools/list",
            Self::ToolsCall => "tools/call",
            Self::ResourcesList => "resources/list",
            Self::ResourcesRead => "resources/read",
            Self::NotificationsInitialized => "notifications/initialized",
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
            "initialize" => Ok(Self::Initialize),
            "tools/list" => Ok(Self::ToolsList),
            "tools/call" => Ok(Self::ToolsCall),
            "resources/list" => Ok(Self::ResourcesList),
            "resources/read" => Ok(Self::ResourcesRead),
            "notifications/initialized" => Ok(Self::NotificationsInitialized),
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

/// MCP protocol version string.
pub const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

/// Server name for MCP initialization.
pub const SERVER_NAME: &str = "bun-docs-mcp-proxy";

/// URI scheme prefix for Bun documentation resources.
pub const BUN_URI_SCHEME: &str = "bun";

/// URI host for Bun documentation resources.
pub const BUN_URI_HOST: &str = "docs";

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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_as_str() {
        assert_eq!(Method::Initialize.as_str(), "initialize");
        assert_eq!(Method::ToolsList.as_str(), "tools/list");
        assert_eq!(Method::ToolsCall.as_str(), "tools/call");
        assert_eq!(Method::ResourcesList.as_str(), "resources/list");
        assert_eq!(Method::ResourcesRead.as_str(), "resources/read");
        assert_eq!(
            Method::NotificationsInitialized.as_str(),
            "notifications/initialized"
        );
    }

    #[test]
    fn method_display() {
        assert_eq!(format!("{}", Method::Initialize), "initialize");
        assert_eq!(format!("{}", Method::ToolsCall), "tools/call");
    }

    #[test]
    fn method_from_str_valid() {
        assert_eq!("initialize".parse::<Method>(), Ok(Method::Initialize));
        assert_eq!("tools/list".parse::<Method>(), Ok(Method::ToolsList));
        assert_eq!("tools/call".parse::<Method>(), Ok(Method::ToolsCall));
        assert_eq!(
            "resources/list".parse::<Method>(),
            Ok(Method::ResourcesList)
        );
        assert_eq!(
            "resources/read".parse::<Method>(),
            Ok(Method::ResourcesRead)
        );
        assert_eq!(
            "notifications/initialized".parse::<Method>(),
            Ok(Method::NotificationsInitialized)
        );
    }

    #[test]
    fn method_from_str_invalid() {
        assert!("unknown".parse::<Method>().is_err());
        assert!("".parse::<Method>().is_err());
        assert!("Initialize".parse::<Method>().is_err()); // case sensitive
    }

    #[test]
    fn content_type_constants() {
        assert_eq!(content_type::JSON, "application/json");
        assert_eq!(content_type::SSE, "text/event-stream");
        assert_eq!(content_type::MARKDOWN, "text/markdown");
    }

    #[test]
    fn protocol_constants() {
        assert_eq!(MCP_PROTOCOL_VERSION, "2024-11-05");
        assert_eq!(SERVER_NAME, "bun-docs-mcp-proxy");
        assert_eq!(BUN_URI_SCHEME, "bun");
        assert_eq!(BUN_URI_HOST, "docs");
        assert_eq!(LINK_MARKER, "Link: ");
    }

    #[test]
    fn error_code_constants() {
        assert_eq!(error_code::PARSE_ERROR, -32700);
        assert_eq!(error_code::INVALID_REQUEST, -32600);
        assert_eq!(error_code::METHOD_NOT_FOUND, -32601);
        assert_eq!(error_code::INVALID_PARAMS, -32602);
        assert_eq!(error_code::INTERNAL_ERROR, -32603);
    }
}
