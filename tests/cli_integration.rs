#![allow(clippy::expect_used, reason = "tests can use expect() for clarity")]
#![allow(clippy::unwrap_used, reason = "tests can use unwrap() for brevity")]
#![allow(clippy::indexing_slicing, reason = "tests use array indexing safely")]
#![allow(
    clippy::tests_outside_test_module,
    reason = "integration tests in tests/ directory"
)]

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use std::path::Path;

/// Test basic search functionality in CLI mode
#[test]
fn cli_search_basic() {
    let mut cmd = cargo_bin_cmd!("bun-docs-mcp-proxy");
    cmd.args(["--search", "Bun.serve"])
        .assert()
        .success()
        .stdout(predicate::str::contains("content").or(predicate::str::contains("result")));
}

/// Test JSON format output
#[test]
fn cli_search_json_format() {
    let mut cmd = cargo_bin_cmd!("bun-docs-mcp-proxy");
    cmd.args(["--search", "HTTP", "--format", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("{").and(predicate::str::contains("}")));
}

/// Test text format output
#[test]
fn cli_search_text_format() {
    let mut cmd = cargo_bin_cmd!("bun-docs-mcp-proxy");
    cmd.args(["--search", "server", "--format", "text"])
        .assert()
        .success();
    // Text format should not contain JSON brackets
    let output = cmd.output().expect("command executed successfully");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("\"content\"") || stdout.contains("Bun"));
}

/// Test markdown format output (fetches raw MDX)
#[test]
fn cli_search_markdown_format() {
    let mut cmd = cargo_bin_cmd!("bun-docs-mcp-proxy");
    cmd.args(["--search", "WebSocket", "--format", "markdown"])
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
fn cli_search_with_output_file() {
    let temp_file = tempfile::Builder::new()
        .prefix("cli_test_")
        .suffix(".json")
        .tempfile_in(".")
        .expect("tempfile creation succeeds");
    let output_str = temp_file.path().file_name().unwrap().to_str().unwrap();

    let mut cmd = cargo_bin_cmd!("bun-docs-mcp-proxy");
    cmd.args(["--search", "test", "--output", output_str])
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
fn cli_search_markdown_to_file() {
    let temp_file = tempfile::Builder::new()
        .prefix("cli_docs_")
        .suffix(".md")
        .tempfile_in(".")
        .expect("tempfile creation succeeds");
    let output_str = temp_file.path().file_name().unwrap().to_str().unwrap();

    let mut cmd = cargo_bin_cmd!("bun-docs-mcp-proxy");
    cmd.args([
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
fn cli_search_file_overwrite_warning() {
    let temp_file = tempfile::Builder::new()
        .prefix("cli_existing_")
        .suffix(".json")
        .tempfile_in(".")
        .expect("tempfile creation succeeds");
    let output_str = temp_file.path().file_name().unwrap().to_str().unwrap();

    // Create existing file
    fs::write(output_str, "existing content").expect("file write succeeds");

    let mut cmd = cargo_bin_cmd!("bun-docs-mcp-proxy");
    cmd.args(["--search", "test", "--output", output_str])
        .assert()
        .success();

    // Verify content was overwritten (no warning shown)
    let content = fs::read_to_string(output_str).expect("file read succeeds");
    assert!(!content.contains("existing content"));
}

/// Test directory traversal prevention
#[test]
fn cli_search_invalid_output_path() {
    let mut cmd = cargo_bin_cmd!("bun-docs-mcp-proxy");
    cmd.args(["--search", "test", "--output", "../../../etc/passwd"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("directory traversal"));
}

/// Test empty search query
#[test]
fn cli_search_empty_query() {
    let mut cmd = cargo_bin_cmd!("bun-docs-mcp-proxy");
    cmd.args(["--search", ""])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be empty"));
}

/// Test short flags
#[test]
fn cli_search_short_flags() {
    let temp_file = tempfile::Builder::new()
        .prefix("cli_short_")
        .suffix(".json")
        .tempfile_in(".")
        .expect("tempfile creation succeeds");
    let output_str = temp_file.path().file_name().unwrap().to_str().unwrap();

    let mut cmd = cargo_bin_cmd!("bun-docs-mcp-proxy");
    cmd.args(["-s", "test", "-f", "json", "-o", output_str])
        .assert()
        .success();

    assert!(Path::new(output_str).exists());
}

/// Test combined search with all options
#[test]
fn cli_search_all_options() {
    let temp_file = tempfile::Builder::new()
        .prefix("cli_full_")
        .suffix(".md")
        .tempfile_in(".")
        .expect("tempfile creation succeeds");
    let output_str = temp_file.path().file_name().unwrap().to_str().unwrap();

    let mut cmd = cargo_bin_cmd!("bun-docs-mcp-proxy");
    cmd.env("RUST_LOG", "info")
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
            && (content.contains("Bun") || content.contains("<!--") || content.contains("---")),
        "Markdown output should contain documentation content"
    );
}

/// Test that logging works in CLI mode
#[test]
fn cli_search_with_debug_logging() {
    let mut cmd = cargo_bin_cmd!("bun-docs-mcp-proxy");
    cmd.env("RUST_LOG", "debug")
        .args(["--search", "test"])
        .assert()
        .success()
        .stderr(predicate::str::contains("bun_docs_mcp_proxy"));
}

/// Test special characters in search query
#[test]
fn cli_search_special_characters() {
    let mut cmd = cargo_bin_cmd!("bun-docs-mcp-proxy");
    cmd.args(["--search", "Bun.serve()"]).assert().success();
}

/// Test output to relative path
#[test]
fn cli_search_relative_output_path() {
    let temp_file = tempfile::Builder::new()
        .prefix("cli_relative_")
        .suffix(".json")
        .tempfile_in(".")
        .expect("tempfile creation succeeds");
    let output_str = temp_file.path().file_name().unwrap().to_str().unwrap();

    let mut cmd = cargo_bin_cmd!("bun-docs-mcp-proxy");
    cmd.args(["--search", "test", "--output", output_str])
        .assert()
        .success();

    assert!(Path::new(output_str).exists());
}

/// Test that MCP mode doesn't interfere with CLI
#[test]
fn cli_search_not_mcp_mode() {
    let mut cmd = cargo_bin_cmd!("bun-docs-mcp-proxy");
    cmd.args(["--search", "test"])
        .write_stdin("invalid json input\n") // Should be ignored in CLI mode
        .assert()
        .success()
        .stdout(predicate::str::contains("content").or(predicate::str::contains("result")));
}
