//! Bun Docs MCP Proxy CLI entry point.
//!
//! Two modes:
//! - **MCP server mode** (default): serves the local Bun documentation index over stdio.
//! - **CLI mode** (`--search ...`): searches Bun's static documentation corpus locally.
//!
//! ## Request Flow (MCP mode)
//!
//! ```text
//! stdin (JSON-RPC) -> local documentation index -> stdout (JSON-RPC)
//! ```
//!
//! Documentation source: Bun's static `llms.txt` and `llms-full.txt` assets.

use std::fs;

use anyhow::Result;
use bun_docs::{
    docs::{DEFAULT_SEARCH_LIMIT, DocsCorpus, SearchHit},
    run_mcp_server,
};
use clap::{Parser, ValueEnum};
use serde::Serialize;
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
    bun-docs --search "Bun.serve"

    # Show three results with matching excerpts
    bun-docs -s "HTTP server" -f text --limit 3

    # Save the best matching page as Markdown
    bun-docs -s "HTTP server" -f markdown --full --limit 1 -o results.md

    # Export as JSON for processing
    bun-docs --search "WebSocket" --format json --output ws-docs.json

    # Run as MCP server (default mode, reads from stdin)
    bun-docs

ENVIRONMENT:
    RUST_LOG    Set logging level (debug, info, warn, error)
                Example: RUST_LOG=debug bun-docs -s "test"

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

    /// Maximum number of search results
    #[arg(short, long, default_value_t = DEFAULT_SEARCH_LIMIT, value_parser = parse_search_limit)]
    limit: usize,

    /// Include complete page bodies instead of compact excerpts (all formats)
    #[arg(long, requires = "search")]
    full: bool,
}

fn parse_search_limit(value: &str) -> Result<usize, String> {
    value
        .parse::<usize>()
        .ok()
        .filter(|&limit| limit > 0)
        .ok_or_else(|| "limit must be a positive integer".to_owned())
}

#[derive(Debug, Serialize)]
struct SearchResult {
    #[serde(flatten)]
    hit: SearchHit,
    excerpt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
}

/// Resets `SIGPIPE` to default behavior on Unix.
///
/// Rust ignores `SIGPIPE` by default, turning broken-pipe writes into `EPIPE` errors.
/// When the parent process (e.g., Zed) closes our stderr pipe, `eprintln!` and
/// tracing writes panic on `EPIPE`, which with `panic = "abort"` produces `SIGABRT`
/// coredumps. Restoring `SIG_DFL` makes the kernel terminate the process cleanly
/// on broken pipe (signal 13, exit code 141) — no panic, no abort, no coredump.
#[cfg(unix)]
#[allow(
    unsafe_code,
    reason = "reset SIGPIPE to prevent coredumps when parent closes stderr pipe"
)]
fn reset_sigpipe() {
    // SAFETY: Restoring SIGPIPE to its default handler (SIG_DFL) is safe.
    // This is the standard Unix behavior before Rust overrides it.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

/// Initializes the `tracing` subscriber for logging.
///
/// This function sets up `tracing_subscriber` to filter logs based on the `RUST_LOG`
/// environment variable (defaulting to `warn` if not set) and directs output to `stderr`.
fn init_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("bun_docs=warn")),
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
/// This function downloads Bun's static documentation sources, builds a local full-text index,
/// formats the ranked results, and writes the output to a file or `stdout`.
///
/// # Arguments
/// * `query` - The search query string.
/// * `format` - The desired `OutputFormat` for the results.
/// * `output_path` - An optional file path to write the output to. If `None`, output is written to `stdout`.
/// * `limit` - Maximum number of results.
/// * `full` - Whether to include complete page bodies.
///
/// # Returns
/// An `anyhow::Result<()>` indicating success or failure.
#[instrument(name = "cli_search", skip(format, output_path), fields(format = ?format, has_output = output_path.is_some()))]
async fn direct_search(
    query: &str,
    format: &OutputFormat,
    output_path: Option<&str>,
    limit: usize,
    full: bool,
) -> Result<()> {
    // Validate query is not empty
    if query.trim().is_empty() {
        return Err(anyhow::anyhow!("Search query cannot be empty"));
    }

    // Validate output path if provided
    if let Some(path) = output_path {
        validate_output_path(path).map_err(|e| anyhow::anyhow!("Invalid output path: {e}"))?;
    }

    let corpus = DocsCorpus::fetch().await?;
    let hits = corpus.search(query, limit)?;
    let results = hits
        .into_iter()
        .map(|hit| {
            let excerpt = corpus.excerpt(query, &hit.url)?;
            let content = full.then(|| {
                corpus
                    .page(&hit.url)
                    .map_or_else(|| hit.description.clone(), |page| page.body.clone())
            });
            Ok(SearchResult {
                hit,
                excerpt,
                content,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    // Format output
    let formatted = match format {
        OutputFormat::Json => serde_json::to_string_pretty(&results)?,
        OutputFormat::Text => format_search_text(&results),
        OutputFormat::Markdown => format_search_markdown(&results),
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

fn format_search_text(results: &[SearchResult]) -> String {
    if results.is_empty() {
        return "No results found.".to_owned();
    }
    results
        .iter()
        .enumerate()
        .map(|(index, result)| {
            let hit = &result.hit;
            let body = result.content.as_deref().unwrap_or(&result.excerpt);
            format!("{}. {}\n   {}\n\n{body}", index + 1, hit.title, hit.url)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn format_search_markdown(results: &[SearchResult]) -> String {
    if results.is_empty() {
        return "No results found.".to_owned();
    }
    results
        .iter()
        .map(|result| {
            let hit = &result.hit;
            let body = result.content.as_deref().unwrap_or(&result.excerpt);
            format!("## [{}]({})\n\n{body}", hit.title, hit.url)
        })
        .collect::<Vec<_>>()
        .join("\n\n---\n\n")
}

#[tokio::main]
async fn main() -> Result<()> {
    #[cfg(unix)]
    reset_sigpipe();

    let cli = Cli::parse();
    init_logging();

    // CLI search mode
    if let Some(query) = &cli.search {
        return direct_search(
            query,
            &cli.format,
            cli.output.as_deref(),
            cli.limit,
            cli.full,
        )
        .await;
    }

    // MCP server mode
    run_mcp_server().await
}

#[cfg(test)]
mod tests {
    #![allow(clippy::assert_is_empty, reason = "test code")]
    #![allow(clippy::expect_used, reason = "tests can use expect()")]
    #![allow(clippy::unwrap_used, reason = "tests can use unwrap()")]
    #![allow(clippy::indexing_slicing, reason = "tests use array indexing")]
    #![allow(clippy::default_numeric_fallback, reason = "test literals")]

    use bun_docs::{JsonRpcRequest, JsonRpcResponse, error_code};
    use serde_json::json;

    use super::*;

    // ============================================================================
    // JSON-RPC Protocol Tests (basic parsing)
    // ============================================================================

    #[test]
    fn test_parse_valid_jsonrpc_request() {
        let message = r#"{"jsonrpc":"2.0","id":1,"method":"server/discover"}"#;
        let request: Result<JsonRpcRequest, _> = serde_json::from_str(message);

        assert!(request.is_ok());
        let req = request.unwrap();
        assert_eq!(req.method, "server/discover");
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
        let result = bun_docs::util::truncate_for_log("hello", 10);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_truncate_for_log_exact_length() {
        let result = bun_docs::util::truncate_for_log("hello", 5);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_truncate_for_log_long_string() {
        let result = bun_docs::util::truncate_for_log("hello world", 5);
        assert_eq!(result, "hello...");
    }

    #[test]
    fn test_truncate_for_log_empty_string() {
        let result = bun_docs::util::truncate_for_log("", 10);
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
        let result =
            direct_search("", &OutputFormat::Json, None, DEFAULT_SEARCH_LIMIT, false).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cannot be empty"));
    }

    #[tokio::test]
    async fn test_direct_search_whitespace_only_query() {
        let result = direct_search(
            "   ",
            &OutputFormat::Json,
            None,
            DEFAULT_SEARCH_LIMIT,
            false,
        )
        .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cannot be empty"));
    }

    #[tokio::test]
    async fn test_direct_search_invalid_output_path() {
        let result = direct_search(
            "test",
            &OutputFormat::Json,
            Some("../output.json"),
            DEFAULT_SEARCH_LIMIT,
            false,
        )
        .await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("directory traversal")
        );
    }

    fn result_fixture(full: bool) -> SearchResult {
        SearchResult {
            hit: SearchHit {
                title: "Server".to_owned(),
                url: "https://bun.com/docs/runtime/http/server".to_owned(),
                description: String::new(),
                score: 1.0,
            },
            excerpt: "Use Bun.serve to start a server...".to_owned(),
            content: full
                .then(|| "Use Bun.serve to start a server.\n\nComplete example.".to_owned()),
        }
    }

    #[test]
    fn compact_formats_include_excerpts_without_full_pages() {
        let results = [result_fixture(false)];
        let text = format_search_text(&results);
        let markdown = format_search_markdown(&results);
        let json = serde_json::to_value(&results).unwrap();
        assert!(text.starts_with("1. Server\n"));
        assert!(markdown.starts_with("## [Server](https://bun.com/docs/runtime/http/server)"));
        for output in [text, markdown] {
            assert!(output.contains("Use Bun.serve to start a server..."));
            assert!(!output.contains("Complete example."));
        }
        assert_eq!(json[0]["excerpt"], results[0].excerpt);
        assert!(json[0].get("content").is_none());
    }

    #[test]
    fn full_pages_are_available_in_every_format() {
        let results = [result_fixture(true)];
        for output in [
            format_search_text(&results),
            format_search_markdown(&results),
        ] {
            assert!(output.contains("Complete example."));
        }
        let json = serde_json::to_value(&results).unwrap();
        assert_eq!(json[0]["content"], results[0].content.as_deref().unwrap());
    }

    #[test]
    fn empty_results_are_explicit_and_json_stays_an_array() {
        assert_eq!(format_search_text(&[]), "No results found.");
        assert_eq!(format_search_markdown(&[]), "No results found.");
        assert_eq!(
            serde_json::to_string(&Vec::<SearchResult>::new()).unwrap(),
            "[]"
        );
    }

    #[test]
    fn cli_limits_and_full_page_options() {
        let cli = Cli::try_parse_from(["bun-docs", "-s", "server", "-l", "1", "--full"]).unwrap();
        assert_eq!(cli.limit, 1);
        assert!(cli.full);
        for value in ["0", "-1", "abc"] {
            Cli::try_parse_from(["bun-docs", "-s", "server", "--limit", value]).unwrap_err();
        }
        Cli::try_parse_from(["bun-docs", "--full"]).unwrap_err();
        Cli::try_parse_from(["bun-docs", "-s", "server", "--format"]).unwrap_err();
    }

    // ============================================================================
    // CLI Integration Tests (require network)
    // ============================================================================

    #[tokio::test]
    #[cfg(feature = "integration-tests")]
    async fn test_direct_search_json_format() {
        let result = direct_search(
            "Bun.serve",
            &OutputFormat::Json,
            None,
            DEFAULT_SEARCH_LIMIT,
            false,
        )
        .await;
        result.unwrap();
    }

    #[tokio::test]
    #[cfg(feature = "integration-tests")]
    async fn test_direct_search_text_format() {
        let result = direct_search(
            "HTTP",
            &OutputFormat::Text,
            None,
            DEFAULT_SEARCH_LIMIT,
            false,
        )
        .await;
        result.unwrap();
    }

    #[tokio::test]
    #[cfg(feature = "integration-tests")]
    async fn test_direct_search_markdown_format() {
        let result = direct_search(
            "server",
            &OutputFormat::Markdown,
            None,
            DEFAULT_SEARCH_LIMIT,
            false,
        )
        .await;
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

        let result = direct_search(
            "test",
            &OutputFormat::Json,
            Some(output_path),
            DEFAULT_SEARCH_LIMIT,
            false,
        )
        .await;
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

        let result = direct_search(
            "Bun",
            &OutputFormat::Markdown,
            Some(output_path),
            DEFAULT_SEARCH_LIMIT,
            false,
        )
        .await;
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
        let result = direct_search(
            "test",
            &OutputFormat::Json,
            Some(output_path),
            DEFAULT_SEARCH_LIMIT,
            false,
        )
        .await;
        result.unwrap();

        // Verify new content
        let content = std::fs::read_to_string(output_path).unwrap();
        assert!(!content.contains("existing content"));

        // tempfile automatically cleans up on drop
    }
}
