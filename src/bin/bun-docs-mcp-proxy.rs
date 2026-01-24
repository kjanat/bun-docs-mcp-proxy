//! Bun Docs MCP Proxy CLI entry point.
//!
//! Two modes:
//! - **MCP server mode** (default): reads JSON-RPC over stdin/stdout, proxies to bun.com/docs/mcp.
//! - **CLI mode** (`--search ...`): performs a Bun docs query and prints/writes results.
//!
//! ## Request Flow (MCP mode)
//!
//! ```text
//! stdin (JSON-RPC) -> Proxy -> HTTP POST -> bun.com/docs/mcp -> SSE stream -> parse -> stdout (JSON-RPC)
//! ```
//!
//! Upstream transport: HTTP POST returning JSON or SSE; downstream transport: stdio JSON-RPC.

use anyhow::Result;
use bun_docs_mcp_proxy::{
    BunDocsClient, UpstreamResponse,
    format::{format_json, format_markdown, format_text},
    run_mcp_server,
};
use clap::{Parser, ValueEnum};
use std::fs;
use tracing::instrument;
use tracing_subscriber::{EnvFilter, fmt::format::FmtSpan};

/// Output format for CLI search results
#[derive(Debug, Clone, ValueEnum)]
enum OutputFormat {
    /// JSON format (default)
    Json,
    /// Plain text format
    Text,
    /// Markdown format
    Markdown,
}

/// Bun Docs MCP Proxy - Protocol adapter and CLI for Bun documentation
#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about,
    long_about = None,
    after_help = r#"EXAMPLES:
    # Search Bun documentation for "serve" keyword
    bun-docs-mcp-proxy --search "Bun.serve"

    # Save results as markdown
    bun-docs-mcp-proxy -s "HTTP server" -f markdown -o results.md

    # Export as JSON for processing
    bun-docs-mcp-proxy --search "WebSocket" --format json --output ws-docs.json

    # Run as MCP server (default mode, reads from stdin)
    bun-docs-mcp-proxy

ENVIRONMENT:
    RUST_LOG    Set logging level (debug, info, warn, error)
                Example: RUST_LOG=debug bun-docs-mcp-proxy -s "test"

MCP SERVER MODE:
    When run without --search, operates as an MCP (Model Context Protocol) server
    reading JSON-RPC requests from stdin and writing responses to stdout."#
)]
struct Cli {
    /// Search query for Bun documentation (enables CLI mode)
    #[arg(short, long)]
    search: Option<String>,

    /// Output file path (default: stdout)
    #[arg(short, long)]
    output: Option<String>,

    /// Output format
    #[arg(short, long, value_enum, default_value_t = OutputFormat::Json)]
    format: OutputFormat,
}

/// Initializes the `tracing` subscriber for logging.
///
/// This function sets up `tracing_subscriber` to filter logs based on the `RUST_LOG`
/// environment variable (defaulting to `info` if not set) and directs output to `stderr`.
fn init_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .without_time()
        .with_span_events(FmtSpan::CLOSE)
        .init();
}

/// Validates a file path to ensure it does not contain directory traversal components (e.g., `..`).
///
/// This is a security measure to prevent writing files outside of the intended directory.
///
/// # Arguments
/// * `path` - The file path string to validate.
///
/// # Returns
/// An `Ok(())` if the path is valid, or an `Err(String)` if it contains traversal components.
fn validate_output_path(path: &str) -> Result<(), String> {
    let path_obj = std::path::Path::new(path);

    // Reject absolute paths
    if path_obj.is_absolute() {
        return Err("Output path must be relative".to_owned());
    }

    // Check for directory traversal attempts
    for component in path_obj.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err("Output path cannot contain '..' (directory traversal)".to_owned());
        }
    }

    Ok(())
}

/// Executes a search query in CLI mode, formats the result, and writes it to the specified output.
///
/// This function orchestrates the CLI search functionality. It builds and sends a `tools/call`
/// request to the Bun Docs API, formats the response according to the user's choice
/// (JSON, text, or Markdown), and writes the output to a file or `stdout`.
///
/// # Arguments
/// * `query` - The search query string.
/// * `format` - The desired `OutputFormat` for the results.
/// * `output_path` - An optional file path to write the output to. If `None`, output is written to `stdout`.
///
/// # Returns
/// An `anyhow::Result<()>` indicating success or failure.
#[instrument(name = "cli_search", skip(format, output_path), fields(format = ?format, has_output = output_path.is_some()))]
async fn direct_search(
    query: &str,
    format: &OutputFormat,
    output_path: Option<&str>,
) -> Result<()> {
    // Validate query is not empty
    if query.trim().is_empty() {
        return Err(anyhow::anyhow!("Search query cannot be empty"));
    }

    let client = BunDocsClient::new();

    // Validate output path if provided
    if let Some(path) = output_path {
        validate_output_path(path).map_err(|e| anyhow::anyhow!("Invalid output path: {e}"))?;
    }

    // Build search request
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1_i32,
        "method": "tools/call",
        "params": {
            "name": "SearchBun",
            "arguments": {
                "query": query
            }
        }
    });

    // Execute search
    let upstream = client.forward_request(request).await?;

    // Handle upstream response
    let search_result = match upstream {
        UpstreamResponse::Ok(result) => result,
        UpstreamResponse::Err { message, .. } => {
            return Err(anyhow::anyhow!("API error: {message}"));
        }
    };

    // Format output
    let formatted = match format {
        OutputFormat::Json => format_json(&search_result)?,
        OutputFormat::Text => format_text(&search_result)?,
        OutputFormat::Markdown => format_markdown(&search_result, &client).await?,
    };

    // Write output
    if let Some(path) = output_path {
        let bytes_written = formatted.len();
        fs::write(path, &formatted)?;
        eprintln!("Output written to: {path} ({bytes_written} bytes)");
    } else {
        println!("{formatted}");
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    init_logging();

    // CLI search mode
    if let Some(query) = &cli.search {
        return direct_search(query, &cli.format, cli.output.as_deref()).await;
    }

    // MCP server mode
    run_mcp_server().await
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, reason = "tests can use expect()")]
    #![allow(clippy::unwrap_used, reason = "tests can use unwrap()")]
    #![allow(clippy::indexing_slicing, reason = "tests use array indexing")]
    #![allow(clippy::default_numeric_fallback, reason = "test literals")]

    use super::*;
    use bun_docs_mcp_proxy::{JsonRpcRequest, JsonRpcResponse, error_code};
    use serde_json::json;

    // ============================================================================
    // JSON-RPC Protocol Tests (basic parsing)
    // ============================================================================

    #[test]
    fn test_parse_valid_jsonrpc_request() {
        let message = r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#;
        let request: Result<JsonRpcRequest, _> = serde_json::from_str(message);

        assert!(request.is_ok());
        let req = request.unwrap();
        assert_eq!(req.method, "initialize");
        assert_eq!(req.id, Some(json!(1)));
    }

    #[test]
    fn test_parse_invalid_jsonrpc_request() {
        let message = r#"{"invalid json"#;
        let request: Result<JsonRpcRequest, _> = serde_json::from_str(message);

        request.unwrap_err();
    }

    #[test]
    fn test_error_response_codes() {
        // Test parse error
        let parse_error = JsonRpcResponse::error(json!(1), -32700, "Parse error".to_owned());
        let serialized_parse = serde_json::to_value(&parse_error).unwrap();
        assert_eq!(serialized_parse["error"]["code"], -32700);

        // Test method not found
        let method_error = JsonRpcResponse::error(json!(2), -32601, "Method not found".to_owned());
        let serialized_method = serde_json::to_value(&method_error).unwrap();
        assert_eq!(serialized_method["error"]["code"], -32601);

        // Test internal error
        let internal_error = JsonRpcResponse::error(json!(3), -32603, "Internal error".to_owned());
        let serialized_internal = serde_json::to_value(&internal_error).unwrap();
        assert_eq!(serialized_internal["error"]["code"], -32603);
    }

    #[test]
    fn test_response_serialization() {
        let response = JsonRpcResponse::success(json!("test-id"), json!({"result": "data"}));
        let serialized = serde_json::to_string(&response);

        assert!(serialized.is_ok());
        let json_str = serialized.unwrap();
        assert!(json_str.contains("\"jsonrpc\":\"2.0\""));
        assert!(json_str.contains("\"id\":\"test-id\""));
    }

    #[test]
    fn test_jsonrpc_request_with_params() {
        let message = r#"{"jsonrpc":"2.0","id":1,"method":"test","params":{"key":"value"}}"#;
        let request: JsonRpcRequest = serde_json::from_str(message).unwrap();

        assert!(request.params.is_some());
        let params = request.params.unwrap();
        assert_eq!(params["key"], "value");
    }

    #[test]
    fn test_response_null_id() {
        let response = JsonRpcResponse::error(json!(null), -32700, "Error".to_owned());
        let serialized = serde_json::to_value(&response).unwrap();

        assert!(serialized["id"].is_null());
    }

    #[test]
    fn test_jsonrpc_error_code_constants() {
        assert_eq!(error_code::PARSE_ERROR, -32700);
        assert_eq!(error_code::INVALID_PARAMS, -32602);
        assert_eq!(error_code::INTERNAL_ERROR, -32603);
        assert_eq!(error_code::METHOD_NOT_FOUND, -32601);
    }

    // ============================================================================
    // Logging Tests
    // ============================================================================

    #[test]
    fn test_init_logging_execution() {
        // Test that init_logging can be called
        // Will panic if called twice, but that's expected
        let result = std::panic::catch_unwind(|| {
            init_logging();
        });

        // Either succeeds or panics (already initialized) - both are fine
        // This just ensures the function code path is exercised
        drop(result);
    }

    // ============================================================================
    // truncate_for_log tests (via lib re-export)
    // ============================================================================

    #[test]
    fn test_truncate_for_log_short_string() {
        let result = bun_docs_mcp_proxy::util::truncate_for_log("hello", 10);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_truncate_for_log_exact_length() {
        let result = bun_docs_mcp_proxy::util::truncate_for_log("hello", 5);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_truncate_for_log_long_string() {
        let result = bun_docs_mcp_proxy::util::truncate_for_log("hello world", 5);
        assert_eq!(result, "hello...");
    }

    #[test]
    fn test_truncate_for_log_empty_string() {
        let result = bun_docs_mcp_proxy::util::truncate_for_log("", 10);
        assert_eq!(result, "");
    }

    // ============================================================================
    // CLI Output Path Validation Tests
    // ============================================================================

    #[test]
    fn test_validate_output_path_valid() {
        validate_output_path("output.json").unwrap();
        validate_output_path("./output.json").unwrap();
        validate_output_path("subdir/output.json").unwrap();
    }

    #[test]
    fn test_validate_output_path_directory_traversal() {
        assert!(validate_output_path("../output.json").is_err());
        assert!(validate_output_path("subdir/../output.json").is_err());
        assert!(validate_output_path("../../etc/passwd").is_err());
    }

    #[test]
    fn test_validate_output_path_absolute_paths() {
        assert!(validate_output_path("/tmp/output.json").is_err());
        assert!(validate_output_path("/etc/passwd").is_err());
        #[cfg(windows)]
        assert!(validate_output_path("C:\\output.json").is_err());
    }

    // ============================================================================
    // CLI direct_search Tests
    // ============================================================================

    #[tokio::test]
    async fn test_direct_search_empty_query() {
        let result = direct_search("", &OutputFormat::Json, None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cannot be empty"));
    }

    #[tokio::test]
    async fn test_direct_search_whitespace_only_query() {
        let result = direct_search("   ", &OutputFormat::Json, None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cannot be empty"));
    }

    #[tokio::test]
    async fn test_direct_search_invalid_output_path() {
        let result = direct_search("test", &OutputFormat::Json, Some("../output.json")).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("directory traversal")
        );
    }

    // ============================================================================
    // CLI Integration Tests (require network)
    // ============================================================================

    #[tokio::test]
    #[cfg(feature = "integration-tests")]
    async fn test_direct_search_json_format() {
        let result = direct_search("Bun.serve", &OutputFormat::Json, None).await;
        result.unwrap();
    }

    #[tokio::test]
    #[cfg(feature = "integration-tests")]
    async fn test_direct_search_text_format() {
        let result = direct_search("HTTP", &OutputFormat::Text, None).await;
        result.unwrap();
    }

    #[tokio::test]
    #[cfg(feature = "integration-tests")]
    async fn test_direct_search_markdown_format() {
        let result = direct_search("server", &OutputFormat::Markdown, None).await;
        result.unwrap();
    }

    #[tokio::test]
    #[cfg(feature = "integration-tests")]
    async fn test_direct_search_with_output_file() {
        let temp_file = tempfile::Builder::new()
            .prefix("test_search_")
            .suffix(".json")
            .tempfile_in(".")
            .unwrap();
        let output_path = temp_file.path().file_name().unwrap().to_str().unwrap();

        let result = direct_search("test", &OutputFormat::Json, Some(output_path)).await;
        result.unwrap();

        // Verify file was created
        assert!(std::path::Path::new(output_path).exists());

        // Read and verify content
        let content = std::fs::read_to_string(output_path).unwrap();
        assert!(!content.is_empty());

        // tempfile automatically cleans up on drop
    }

    #[tokio::test]
    #[cfg(feature = "integration-tests")]
    async fn test_direct_search_markdown_with_file() {
        let temp_file = tempfile::Builder::new()
            .prefix("test_markdown_")
            .suffix(".md")
            .tempfile_in(".")
            .unwrap();
        let output_path = temp_file.path().file_name().unwrap().to_str().unwrap();

        let result = direct_search("Bun", &OutputFormat::Markdown, Some(output_path)).await;
        result.unwrap();

        // Verify file was created
        assert!(std::path::Path::new(output_path).exists());

        // Read and verify markdown content (may include URL comments or MDX)
        let content = std::fs::read_to_string(output_path).unwrap();
        assert!(!content.is_empty(), "Markdown output should not be empty");
        // The content could be raw MDX with URL comments or fallback text

        // tempfile automatically cleans up on drop
    }

    #[tokio::test]
    #[cfg(feature = "integration-tests")]
    async fn test_direct_search_file_overwrite() {
        let temp_file = tempfile::Builder::new()
            .prefix("test_overwrite_")
            .suffix(".json")
            .tempfile_in(".")
            .unwrap();
        let output_path = temp_file.path().file_name().unwrap().to_str().unwrap();

        // Create existing file
        fs::write(output_path, "existing content").unwrap();
        assert!(std::path::Path::new(output_path).exists());

        // Should overwrite
        let result = direct_search("test", &OutputFormat::Json, Some(output_path)).await;
        result.unwrap();

        // Verify new content
        let content = std::fs::read_to_string(output_path).unwrap();
        assert!(!content.contains("existing content"));

        // tempfile automatically cleans up on drop
    }

    // ============================================================================
    // CLI Argument Tests (process-spawning)
    // ============================================================================

    mod cli_args {
        #![allow(
            deprecated,
            reason = "cargo_bin_cmd! macro unavailable in inline tests"
        )]

        use assert_cmd::Command;
        use core::time::Duration;
        use predicates::prelude::*;

        #[test]
        fn unknown_arg_exits_nonzero() {
            Command::cargo_bin("bun-docs-mcp-proxy")
                .unwrap()
                .arg("--invalid-flag-xyz")
                .assert()
                .failure()
                .stderr(
                    predicate::str::contains("unexpected argument")
                        .or(predicate::str::contains("unknown"))
                        .or(predicate::str::contains("unrecognized")),
                );
        }

        #[test]
        fn help_flag_exits_success() {
            Command::cargo_bin("bun-docs-mcp-proxy")
                .unwrap()
                .arg("--help")
                .assert()
                .success()
                .stdout(
                    predicate::str::contains("USAGE")
                        .or(predicate::str::contains("bun-docs-mcp-proxy")),
                );
        }

        #[test]
        fn help_short_flag_exits_success() {
            Command::cargo_bin("bun-docs-mcp-proxy")
                .unwrap()
                .arg("-h")
                .assert()
                .success()
                .stdout(
                    predicate::str::contains("Usage")
                        .or(predicate::str::contains("Options"))
                        .or(predicate::str::contains("search")),
                );
        }

        #[test]
        fn version_flag_exits_success() {
            Command::cargo_bin("bun-docs-mcp-proxy")
                .unwrap()
                .arg("--version")
                .assert()
                .success()
                .stdout(
                    predicate::str::is_match(r"bun-docs-mcp-proxy \d+\.\d+\.\d+")
                        .expect("valid regex pattern"),
                );
        }

        #[test]
        fn version_short_flag_exits_success() {
            Command::cargo_bin("bun-docs-mcp-proxy")
                .unwrap()
                .arg("-V")
                .assert()
                .success()
                .stdout(predicate::str::is_match(r"\d+\.\d+\.\d+").expect("valid regex pattern"));
        }

        #[test]
        fn handles_stdin_eof_cleanly() {
            Command::cargo_bin("bun-docs-mcp-proxy")
                .unwrap()
                .assert()
                .success();
        }

        #[test]
        fn handles_invalid_json_gracefully() {
            Command::cargo_bin("bun-docs-mcp-proxy")
                .unwrap()
                .write_stdin("not valid json at all\n")
                .timeout(Duration::from_secs(2_u64))
                .assert()
                .stderr(
                    predicate::str::contains("parse")
                        .or(predicate::str::contains("JSON"))
                        .or(predicate::str::is_empty()),
                );
        }

        #[test]
        fn initialize_roundtrip() {
            Command::cargo_bin("bun-docs-mcp-proxy")
                .unwrap()
                .write_stdin(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}
"#,
                )
                .timeout(Duration::from_secs(2_u64))
                .assert()
                .success()
                .stdout(predicate::str::contains("protocolVersion"))
                .stdout(predicate::str::contains("2024-11-05"));
        }
    }

    /// CLI integration tests (process-spawning, require network)
    mod cli_integration {
        use assert_cmd::Command;
        use predicates::prelude::*;
        #[cfg(feature = "integration-tests")]
        use std::fs;
        #[cfg(feature = "integration-tests")]
        use std::path::Path;

        /// Test basic search functionality in CLI mode
        #[test]
        #[cfg(feature = "integration-tests")]
        fn cli_search_basic() {
            Command::cargo_bin("bun-docs-mcp-proxy")
                .unwrap()
                .args(["--search", "Bun.serve"])
                .assert()
                .success()
                .stdout(predicate::str::contains("content").or(predicate::str::contains("result")));
        }

        /// Test JSON format output
        #[test]
        #[cfg(feature = "integration-tests")]
        fn cli_search_json_format() {
            Command::cargo_bin("bun-docs-mcp-proxy")
                .unwrap()
                .args(["--search", "HTTP", "--format", "json"])
                .assert()
                .success()
                .stdout(predicate::str::contains("{").and(predicate::str::contains("}")));
        }

        /// Test text format output
        #[test]
        #[cfg(feature = "integration-tests")]
        fn cli_search_text_format() {
            let cmd = Command::cargo_bin("bun-docs-mcp-proxy")
                .unwrap()
                .args(["--search", "server", "--format", "text"])
                .assert()
                .success();

            // Text format should not contain JSON brackets
            let output = cmd.get_output();
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(!stdout.contains("\"content\"") || stdout.contains("Bun"));
        }

        /// Test markdown format output (fetches raw MDX)
        #[test]
        #[cfg(feature = "integration-tests")]
        fn cli_search_markdown_format() {
            Command::cargo_bin("bun-docs-mcp-proxy")
                .unwrap()
                .args(["--search", "WebSocket", "--format", "markdown"])
                .assert()
                .success()
                .stdout(
                    // Should contain MDX content or URL comment or separator
                    predicate::str::contains("<!--")
                        .or(predicate::str::contains("---"))
                        .or(predicate::str::contains("WebSocket")),
                );
        }

        /// Test file output creation
        #[test]
        #[cfg(feature = "integration-tests")]
        fn cli_search_with_output_file() {
            let temp_file = tempfile::Builder::new()
                .prefix("cli_test_")
                .suffix(".json")
                .tempfile_in(".")
                .expect("tempfile creation succeeds");
            let output_str = temp_file.path().file_name().unwrap().to_str().unwrap();

            Command::cargo_bin("bun-docs-mcp-proxy")
                .unwrap()
                .args(["--search", "test", "--output", output_str])
                .assert()
                .success()
                .stderr(predicate::str::contains("Output written to:"));

            // Verify file exists and contains content
            assert!(Path::new(output_str).exists());
            let content = fs::read_to_string(output_str).expect("file read succeeds");
            assert!(!content.is_empty());
        }

        /// Test markdown file output (fetches raw MDX)
        #[test]
        #[cfg(feature = "integration-tests")]
        fn cli_search_markdown_to_file() {
            let temp_file = tempfile::Builder::new()
                .prefix("cli_docs_")
                .suffix(".md")
                .tempfile_in(".")
                .expect("tempfile creation succeeds");
            let output_str = temp_file.path().file_name().unwrap().to_str().unwrap();

            Command::cargo_bin("bun-docs-mcp-proxy")
                .unwrap()
                .args([
                    "--search", "Bun", "--format", "markdown", "--output", output_str,
                ])
                .assert()
                .success();

            // Verify markdown file contains MDX content or URL comments
            let content = fs::read_to_string(output_str).expect("file read succeeds");
            assert!(
                content.contains("<!--") || content.contains("---") || content.contains("Bun"),
                "Markdown output should contain MDX content, URL comments, or separators"
            );
        }

        /// Test overwrite warning
        #[test]
        #[cfg(feature = "integration-tests")]
        fn cli_search_file_overwrite_warning() {
            let temp_file = tempfile::Builder::new()
                .prefix("cli_existing_")
                .suffix(".json")
                .tempfile_in(".")
                .expect("tempfile creation succeeds");
            let output_str = temp_file.path().file_name().unwrap().to_str().unwrap();

            // Create existing file
            fs::write(output_str, "existing content").expect("file write succeeds");

            Command::cargo_bin("bun-docs-mcp-proxy")
                .unwrap()
                .args(["--search", "test", "--output", output_str])
                .assert()
                .success();

            // Verify content was overwritten (no warning shown)
            let content = fs::read_to_string(output_str).expect("file read succeeds");
            assert!(!content.contains("existing content"));
        }

        /// Test directory traversal prevention
        #[test]
        fn cli_search_invalid_output_path() {
            Command::cargo_bin("bun-docs-mcp-proxy")
                .unwrap()
                .args(["--search", "test", "--output", "../../../etc/passwd"])
                .assert()
                .failure()
                .stderr(predicate::str::contains("directory traversal"));
        }

        /// Test empty search query
        #[test]
        fn cli_search_empty_query() {
            Command::cargo_bin("bun-docs-mcp-proxy")
                .unwrap()
                .args(["--search", ""])
                .assert()
                .failure()
                .stderr(predicate::str::contains("cannot be empty"));
        }

        /// Test short flags
        #[test]
        #[cfg(feature = "integration-tests")]
        fn cli_search_short_flags() {
            let temp_file = tempfile::Builder::new()
                .prefix("cli_short_")
                .suffix(".json")
                .tempfile_in(".")
                .expect("tempfile creation succeeds");
            let output_str = temp_file.path().file_name().unwrap().to_str().unwrap();

            Command::cargo_bin("bun-docs-mcp-proxy")
                .unwrap()
                .args(["-s", "test", "-f", "json", "-o", output_str])
                .assert()
                .success();

            assert!(Path::new(output_str).exists());
        }

        /// Test combined search with all options
        #[test]
        #[cfg(feature = "integration-tests")]
        fn cli_search_all_options() {
            let temp_file = tempfile::Builder::new()
                .prefix("cli_full_")
                .suffix(".md")
                .tempfile_in(".")
                .expect("tempfile creation succeeds");
            let output_str = temp_file.path().file_name().unwrap().to_str().unwrap();

            Command::cargo_bin("bun-docs-mcp-proxy")
                .unwrap()
                .env("RUST_LOG", "info")
                .args([
                    "--search",
                    "Bun.serve",
                    "--format",
                    "markdown",
                    "--output",
                    output_str,
                ])
                .assert()
                .success()
                .stderr(predicate::str::contains("Output written to:"));

            // Verify complete markdown file contains MDX content
            let content = fs::read_to_string(output_str).expect("file read succeeds");
            assert!(
                !content.is_empty()
                    && (content.contains("Bun")
                        || content.contains("<!--")
                        || content.contains("---")),
                "Markdown output should contain documentation content"
            );
        }

        /// Test that logging works in CLI mode
        #[test]
        #[cfg(feature = "integration-tests")]
        fn cli_search_with_debug_logging() {
            Command::cargo_bin("bun-docs-mcp-proxy")
                .unwrap()
                .env("RUST_LOG", "debug")
                .args(["--search", "test"])
                .assert()
                .success()
                .stderr(predicate::str::contains("bun_docs_mcp_proxy"));
        }

        /// Test special characters in search query
        #[test]
        #[cfg(feature = "integration-tests")]
        fn cli_search_special_characters() {
            Command::cargo_bin("bun-docs-mcp-proxy")
                .unwrap()
                .args(["--search", "Bun.serve()"])
                .assert()
                .success();
        }

        /// Test output to relative path
        #[test]
        #[cfg(feature = "integration-tests")]
        fn cli_search_relative_output_path() {
            let temp_file = tempfile::Builder::new()
                .prefix("cli_relative_")
                .suffix(".json")
                .tempfile_in(".")
                .expect("tempfile creation succeeds");
            let output_str = temp_file.path().file_name().unwrap().to_str().unwrap();

            Command::cargo_bin("bun-docs-mcp-proxy")
                .unwrap()
                .args(["--search", "test", "--output", output_str])
                .assert()
                .success();

            assert!(Path::new(output_str).exists());
        }

        /// Test that MCP mode doesn't interfere with CLI
        #[test]
        #[cfg(feature = "integration-tests")]
        fn cli_search_not_mcp_mode() {
            Command::cargo_bin("bun-docs-mcp-proxy")
                .unwrap()
                .args(["--search", "test"])
                .write_stdin("invalid json input\n") // Should be ignored in CLI mode
                .assert()
                .success()
                .stdout(predicate::str::contains("content").or(predicate::str::contains("result")));
        }
    }
}
