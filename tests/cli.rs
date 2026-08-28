//! CLI integration tests for bun-docs-mcp-proxy.
#![allow(clippy::tests_outside_test_module, reason = "integration test file")]

use core::time::Duration;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

#[test]
fn stdin_parse_error() {
    cargo_bin_cmd!("bun-docs-mcp-proxy")
        .write_stdin("{ invalid json without closing\n")
        .timeout(Duration::from_secs(2_u64))
        .assert()
        .stderr(
            predicate::str::contains("parse")
                .or(predicate::str::contains("Parse error"))
                .or(predicate::str::contains("EOF")),
        );
}

#[test]
fn unknown_arg_exits_nonzero() {
    cargo_bin_cmd!("bun-docs-mcp-proxy")
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
    cargo_bin_cmd!("bun-docs-mcp-proxy")
        .arg("--help")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("USAGE").or(predicate::str::contains("bun-docs-mcp-proxy")),
        );
}

#[test]
fn help_short_flag_exits_success() {
    cargo_bin_cmd!("bun-docs-mcp-proxy")
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
    cargo_bin_cmd!("bun-docs-mcp-proxy")
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("bun-docs-mcp-proxy"));
}

#[test]
fn version_short_flag_exits_success() {
    cargo_bin_cmd!("bun-docs-mcp-proxy")
        .arg("-V")
        .assert()
        .success()
        .stdout(predicate::str::contains("2.0.0"));
}

#[test]
fn handles_stdin_eof_cleanly() {
    cargo_bin_cmd!("bun-docs-mcp-proxy").assert().success();
}

#[test]
fn handles_invalid_json_gracefully() {
    cargo_bin_cmd!("bun-docs-mcp-proxy")
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
    cargo_bin_cmd!("bun-docs-mcp-proxy")
        .write_stdin(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}
"#,
        )
        .timeout(Duration::from_secs(2_u64))
        .assert()
        .success()
        .stdout(predicate::str::contains("protocolVersion"));
}

#[test]
fn cli_search_invalid_output_path() {
    cargo_bin_cmd!("bun-docs-mcp-proxy")
        .args(["--search", "test", "--output", "../../../etc/passwd"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("directory traversal"));
}

#[test]
fn cli_search_empty_query() {
    cargo_bin_cmd!("bun-docs-mcp-proxy")
        .args(["--search", ""])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be empty"));
}

#[cfg(feature = "integration-tests")]
fn temp_name(
    prefix: &str,
    suffix: &str,
) -> anyhow::Result<(tempfile::NamedTempFile, std::path::PathBuf)> {
    let temp_file = tempfile::Builder::new()
        .prefix(prefix)
        .suffix(suffix)
        .tempfile_in(".")?;
    let name = temp_file
        .path()
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("tempfile missing file_name"))?
        .to_owned();
    Ok((temp_file, std::path::PathBuf::from(name)))
}

#[test]
#[cfg(feature = "integration-tests")]
fn cli_search_basic() {
    cargo_bin_cmd!("bun-docs-mcp-proxy")
        .args(["--search", "Bun.serve"])
        .assert()
        .success()
        .stdout(predicate::str::contains("content").or(predicate::str::contains("result")));
}

#[test]
#[cfg(feature = "integration-tests")]
fn cli_search_json_format() {
    cargo_bin_cmd!("bun-docs-mcp-proxy")
        .args(["--search", "HTTP", "--format", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("{").and(predicate::str::contains("}")));
}

#[test]
#[cfg(feature = "integration-tests")]
fn cli_search_text_format() {
    let cmd = cargo_bin_cmd!("bun-docs-mcp-proxy")
        .args(["--search", "server", "--format", "text"])
        .assert()
        .success();

    let output = cmd.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("\"content\"") || stdout.contains("Bun"));
}

#[test]
#[cfg(feature = "integration-tests")]
fn cli_search_markdown_format() {
    cargo_bin_cmd!("bun-docs-mcp-proxy")
        .args(["--search", "WebSocket", "--format", "markdown"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("<!--")
                .or(predicate::str::contains("---"))
                .or(predicate::str::contains("WebSocket")),
        );
}

#[test]
#[cfg(feature = "integration-tests")]
fn cli_search_with_output_file() -> anyhow::Result<()> {
    let (_temp_file, output_path) = temp_name("cli_test_", ".json")?;
    let output_str = output_path.to_string_lossy();

    cargo_bin_cmd!("bun-docs-mcp-proxy")
        .args(["--search", "test", "--output", output_str.as_ref()])
        .assert()
        .success()
        .stderr(predicate::str::contains("Output written to:"));

    anyhow::ensure!(output_path.exists(), "output file missing");
    let content = std::fs::read_to_string(&output_path)?;
    anyhow::ensure!(!content.is_empty(), "output file empty");
    Ok(())
}

#[test]
#[cfg(feature = "integration-tests")]
fn cli_search_markdown_to_file() -> anyhow::Result<()> {
    let (_temp_file, output_path) = temp_name("cli_docs_", ".md")?;
    let output_str = output_path.to_string_lossy();

    cargo_bin_cmd!("bun-docs-mcp-proxy")
        .args([
            "--search",
            "Bun",
            "--format",
            "markdown",
            "--output",
            output_str.as_ref(),
        ])
        .assert()
        .success();

    let content = std::fs::read_to_string(&output_path)?;
    anyhow::ensure!(
        content.contains("<!--") || content.contains("---") || content.contains("Bun"),
        "markdown output missing MDX content, URL comments, or separators"
    );
    Ok(())
}

#[test]
#[cfg(feature = "integration-tests")]
fn cli_search_file_overwrite_warning() -> anyhow::Result<()> {
    let (_temp_file, output_path) = temp_name("cli_existing_", ".json")?;
    let output_str = output_path.to_string_lossy();

    std::fs::write(&output_path, "existing content")?;

    cargo_bin_cmd!("bun-docs-mcp-proxy")
        .args(["--search", "test", "--output", output_str.as_ref()])
        .assert()
        .success();

    let content = std::fs::read_to_string(&output_path)?;
    anyhow::ensure!(
        !content.contains("existing content"),
        "output still has previous content"
    );
    Ok(())
}

#[test]
#[cfg(feature = "integration-tests")]
fn cli_search_short_flags() -> anyhow::Result<()> {
    let (_temp_file, output_path) = temp_name("cli_short_", ".json")?;
    let output_str = output_path.to_string_lossy();

    cargo_bin_cmd!("bun-docs-mcp-proxy")
        .args(["-s", "test", "-f", "json", "-o", output_str.as_ref()])
        .assert()
        .success();

    anyhow::ensure!(output_path.exists(), "output file missing");
    Ok(())
}

#[test]
#[cfg(feature = "integration-tests")]
fn cli_search_all_options() -> anyhow::Result<()> {
    let (_temp_file, output_path) = temp_name("cli_full_", ".md")?;
    let output_str = output_path.to_string_lossy();

    cargo_bin_cmd!("bun-docs-mcp-proxy")
        .env("RUST_LOG", "info")
        .args([
            "--search",
            "Bun.serve",
            "--format",
            "markdown",
            "--output",
            output_str.as_ref(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("Output written to:"));

    let content = std::fs::read_to_string(&output_path)?;
    anyhow::ensure!(
        !content.is_empty()
            && (content.contains("Bun") || content.contains("<!--") || content.contains("---")),
        "markdown output missing documentation content"
    );
    Ok(())
}

#[test]
#[cfg(feature = "integration-tests")]
fn cli_search_with_debug_logging() {
    cargo_bin_cmd!("bun-docs-mcp-proxy")
        .env("RUST_LOG", "debug")
        .args(["--search", "test"])
        .assert()
        .success()
        .stderr(predicate::str::contains("bun_docs_mcp_proxy"));
}

#[test]
#[cfg(feature = "integration-tests")]
fn cli_search_special_characters() {
    cargo_bin_cmd!("bun-docs-mcp-proxy")
        .args(["--search", "Bun.serve()"])
        .assert()
        .success();
}

#[test]
#[cfg(feature = "integration-tests")]
fn cli_search_relative_output_path() -> anyhow::Result<()> {
    let (_temp_file, output_path) = temp_name("cli_relative_", ".json")?;
    let output_str = output_path.to_string_lossy();

    cargo_bin_cmd!("bun-docs-mcp-proxy")
        .args(["--search", "test", "--output", output_str.as_ref()])
        .assert()
        .success();

    anyhow::ensure!(output_path.exists(), "output file missing");
    Ok(())
}

#[test]
#[cfg(feature = "integration-tests")]
fn cli_search_not_mcp_mode() {
    cargo_bin_cmd!("bun-docs-mcp-proxy")
        .args(["--search", "test"])
        .write_stdin("invalid json input\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("content").or(predicate::str::contains("result")));
}
