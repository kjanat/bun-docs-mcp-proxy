use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

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
    let output = cmd.output().unwrap();
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
    let temp_dir = tempdir().unwrap();
    let output_path = temp_dir.path().join("test_output.json");
    let output_str = output_path.to_str().unwrap();

    let mut cmd = cargo_bin_cmd!("bun-docs-mcp-proxy");
    cmd.args(["--search", "test", "--output", output_str])
        .assert()
        .success()
        .stderr(predicate::str::contains("Output written to:"));

    // Verify file exists and contains content
    assert!(output_path.exists());
    let content = fs::read_to_string(&output_path).unwrap();
    assert!(!content.is_empty());
}

/// Test markdown file output (fetches raw MDX)
#[test]
fn cli_search_markdown_to_file() {
    let temp_dir = tempdir().unwrap();
    let output_path = temp_dir.path().join("docs.md");
    let output_str = output_path.to_str().unwrap();

    let mut cmd = cargo_bin_cmd!("bun-docs-mcp-proxy");
    cmd.args([
        "--search", "Bun", "--format", "markdown", "--output", output_str,
    ])
    .assert()
    .success();

    // Verify markdown file contains MDX content or URL comments
    let content = fs::read_to_string(&output_path).unwrap();
    assert!(
        content.contains("<!--") || content.contains("---") || content.contains("Bun"),
        "Markdown output should contain MDX content, URL comments, or separators"
    );
}

/// Test overwrite warning
#[test]
fn cli_search_file_overwrite_warning() {
    let temp_dir = tempdir().unwrap();
    let output_path = temp_dir.path().join("existing.json");
    let output_str = output_path.to_str().unwrap();

    // Create existing file
    fs::write(&output_path, "existing content").unwrap();

    let mut cmd = cargo_bin_cmd!("bun-docs-mcp-proxy");
    cmd.args(["--search", "test", "--output", output_str])
        .assert()
        .success()
        .stderr(predicate::str::contains("Warning").and(predicate::str::contains("overwritten")));

    // Verify content was overwritten
    let content = fs::read_to_string(&output_path).unwrap();
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
    cmd.args(["--search", ""]).assert().success(); // API handles empty queries
}

/// Test short flags
#[test]
fn cli_search_short_flags() {
    let temp_dir = tempdir().unwrap();
    let output_path = temp_dir.path().join("short.json");
    let output_str = output_path.to_str().unwrap();

    let mut cmd = cargo_bin_cmd!("bun-docs-mcp-proxy");
    cmd.args(["-s", "test", "-f", "json", "-o", output_str])
        .assert()
        .success();

    assert!(output_path.exists());
}

/// Test combined search with all options
#[test]
fn cli_search_all_options() {
    let temp_dir = tempdir().unwrap();
    let output_path = temp_dir.path().join("full_test.md");
    let output_str = output_path.to_str().unwrap();

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
    let content = fs::read_to_string(&output_path).unwrap();
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
    let temp_dir = tempdir().unwrap();
    let old_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(&temp_dir).unwrap();

    let mut cmd = cargo_bin_cmd!("bun-docs-mcp-proxy");
    cmd.args(["--search", "test", "--output", "./output.json"])
        .assert()
        .success();

    assert!(Path::new("./output.json").exists());

    // Cleanup
    std::env::set_current_dir(old_dir).unwrap();
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
