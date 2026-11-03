//! CLI integration tests for bun-docs.
#![allow(clippy::tests_outside_test_module, reason = "integration test file")]

use core::time::Duration;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

#[test]
fn mcp_validates_each_request_without_a_handshake() -> anyhow::Result<()> {
    use serde_json::json;

    let meta = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {}
    });
    let requests = [
        json!({"jsonrpc":"2.0", "id":1_i32, "method":"server/discover", "params":{"_meta":meta}}),
        json!({"jsonrpc":"2.0", "id":2_i32, "method":"tools/list", "params":{"_meta":meta}}),
        json!({"jsonrpc":"2.0", "id":3_i32, "method":"tools/list"}),
        json!({"jsonrpc":"2.0", "id":4_i32, "method":"server/discover", "params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"1900-01-01", "io.modelcontextprotocol/clientCapabilities":{}}}}),
        json!({"jsonrpc":"2.0", "id":5_i32, "method":"tools/list", "params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}}),
        json!({"jsonrpc":"2.0", "id":6_i32, "method":"tools/list", "params":{"_meta":{"io.modelcontextprotocol/clientCapabilities":{}}}}),
        json!({"jsonrpc":"2.0", "id":7_i32, "method":"initialize"}),
        json!({"jsonrpc":"2.0", "id":null, "method":"tools/list", "params":{"_meta":meta}}),
        json!({"jsonrpc":"2.0", "id":true, "method":"tools/list", "params":{"_meta":meta}}),
        json!({"jsonrpc":"2.0", "id":1.5_f64, "method":"tools/list", "params":{"_meta":meta}}),
        json!({"jsonrpc":"2.0", "method":"notifications/cancelled", "params":{"requestId":1_i32}}),
        json!({"jsonrpc":"2.0", "id":8_i32, "method":"unknown", "params":{"_meta":meta}}),
        json!({"jsonrpc":"2.0", "id":"last", "method":"tools/list", "params":{"_meta":meta}}),
    ];
    let mut input = requests
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    input.push('\n');
    let assertion = cargo_bin_cmd!("bun-docs")
        .write_stdin(input)
        .timeout(Duration::from_secs(2_u64))
        .assert()
        .success();
    let parsed_responses = String::from_utf8(assertion.get_output().stdout.clone())?
        .lines()
        .map(serde_json::from_str::<serde_json::Value>)
        .collect::<Result<Vec<_>, _>>()?;
    let mut responses = parsed_responses.iter();
    let discovery = responses
        .next()
        .ok_or_else(|| anyhow::anyhow!("missing discovery"))?;
    anyhow::ensure!(discovery.pointer("/result/supportedVersions") == Some(&json!(["2026-07-28"])));
    anyhow::ensure!(
        discovery.pointer("/result/_meta/io.modelcontextprotocol~1serverInfo/name")
            == Some(&json!("bun-docs"))
    );
    let tools = responses
        .next()
        .ok_or_else(|| anyhow::anyhow!("missing tool list"))?;
    anyhow::ensure!(tools.pointer("/result/resultType") == Some(&json!("complete")));
    anyhow::ensure!(tools.pointer("/result/tools/0/name") == Some(&json!("search_bun")));
    for code in [
        -32602_i32, -32022_i32, -32602_i32, -32602_i32, -32601_i32, -32600_i32, -32600_i32,
        -32600_i32, -32601_i32,
    ] {
        let response = responses
            .next()
            .ok_or_else(|| anyhow::anyhow!("missing error"))?;
        anyhow::ensure!(
            response.pointer("/error/code") == Some(&json!(code)),
            "unexpected error: {response}"
        );
        if code == -32022_i32 {
            anyhow::ensure!(
                response.pointer("/error/data")
                    == Some(&json!({"requested":"1900-01-01", "supported":["2026-07-28"]}))
            );
        }
    }
    let last = responses
        .next()
        .ok_or_else(|| anyhow::anyhow!("missing final reply"))?;
    anyhow::ensure!(
        last.get("id") == Some(&json!("last"))
            && last.pointer("/result/resultType") == Some(&json!("complete"))
    );
    anyhow::ensure!(
        responses.next().is_none(),
        "notifications must not produce replies"
    );
    Ok(())
}

#[test]
#[cfg(feature = "integration-tests")]
fn mcp_search_uses_current_protocol_without_discovery() -> anyhow::Result<()> {
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": "search",
        "method": "tools/call",
        "params": {
            "name": "search_bun",
            "arguments": { "query": "Bun.serve", "limit": 1_i32 },
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientCapabilities": {}
            }
        }
    });
    let assertion = cargo_bin_cmd!("bun-docs")
        .write_stdin(format!("{request}\n"))
        .timeout(Duration::from_secs(30_u64))
        .assert()
        .success();
    let response: serde_json::Value = serde_json::from_slice(&assertion.get_output().stdout)?;
    anyhow::ensure!(response.get("id") == Some(&serde_json::json!("search")));
    anyhow::ensure!(response.pointer("/result/resultType") == Some(&serde_json::json!("complete")));
    anyhow::ensure!(
        response
            .pointer("/result/content")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|items| items.len() == 1)
    );
    Ok(())
}

#[test]
fn stdin_parse_error() {
    cargo_bin_cmd!("bun-docs")
        .write_stdin("{ invalid json without closing\n")
        .timeout(Duration::from_secs(10_u64))
        .assert()
        .stderr(
            predicate::str::contains("parse")
                .or(predicate::str::contains("Parse error"))
                .or(predicate::str::contains("EOF")),
        );
}

#[test]
fn unknown_arg_exits_nonzero() {
    cargo_bin_cmd!("bun-docs")
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
    cargo_bin_cmd!("bun-docs")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("USAGE").or(predicate::str::contains("bun-docs")));
}

#[test]
fn help_short_flag_exits_success() {
    cargo_bin_cmd!("bun-docs")
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
    cargo_bin_cmd!("bun-docs")
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("bun-docs"));
}

#[test]
fn version_short_flag_exits_success() {
    cargo_bin_cmd!("bun-docs")
        .arg("-V")
        .assert()
        .success()
        .stdout(predicate::str::contains("2.0.0"));
}

#[test]
fn handles_stdin_eof_cleanly() {
    cargo_bin_cmd!("bun-docs").assert().success();
}

#[test]
fn handles_invalid_json_gracefully() {
    cargo_bin_cmd!("bun-docs")
        .write_stdin("not valid json at all\n")
        .timeout(Duration::from_secs(10_u64))
        .assert()
        .stderr(
            predicate::str::contains("parse")
                .or(predicate::str::contains("JSON"))
                .or(predicate::str::is_empty()),
        );
}

#[test]
fn discovery_roundtrip() {
    cargo_bin_cmd!("bun-docs")
        .write_stdin(
            r#"{"jsonrpc":"2.0","id":1,"method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}
"#,
        )
        .timeout(Duration::from_secs(10_u64))
        .assert()
        .success()
        .stdout(predicate::str::contains("supportedVersions").and(predicate::str::contains("2026-07-28")));
}

#[test]
fn cli_search_invalid_output_path() {
    cargo_bin_cmd!("bun-docs")
        .args(["--search", "test", "--output", "../../../etc/passwd"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("directory traversal"));
}

#[test]
fn cli_search_empty_query() {
    cargo_bin_cmd!("bun-docs")
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
    cargo_bin_cmd!("bun-docs")
        .env_remove("RUST_LOG")
        .args(["--search", "Bun.serve"])
        .assert()
        .success()
        .stderr(predicate::str::is_empty())
        .stdout(predicate::str::contains("\"title\""));
}

#[test]
#[cfg(feature = "integration-tests")]
fn cli_search_limit_and_full_content() -> anyhow::Result<()> {
    let assertion = cargo_bin_cmd!("bun-docs")
        .args(["--search", "server", "--limit", "1", "--full"])
        .assert()
        .success();
    let results: Vec<serde_json::Value> = serde_json::from_slice(&assertion.get_output().stdout)?;
    anyhow::ensure!(results.len() == 1, "limit must select one result");
    let result = results
        .first()
        .ok_or_else(|| anyhow::anyhow!("missing result"))?;
    anyhow::ensure!(
        result["excerpt"]
            .as_str()
            .is_some_and(|text| !text.is_empty()),
        "missing excerpt"
    );
    anyhow::ensure!(
        result["content"]
            .as_str()
            .is_some_and(|text| text.len() > 1000),
        "missing complete page"
    );
    Ok(())
}

#[test]
#[cfg(feature = "integration-tests")]
fn cli_search_json_format() {
    cargo_bin_cmd!("bun-docs")
        .args(["--search", "HTTP", "--format", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("{").and(predicate::str::contains("}")));
}

#[test]
#[cfg(feature = "integration-tests")]
fn cli_search_text_format() {
    let cmd = cargo_bin_cmd!("bun-docs")
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
    let assertion = cargo_bin_cmd!("bun-docs")
        .args(["--search", "WebSocket", "--format", "markdown"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("<!--")
                .or(predicate::str::contains("---"))
                .or(predicate::str::contains("WebSocket")),
        );
    let stdout = String::from_utf8_lossy(&assertion.get_output().stdout);
    assert!(
        stdout.lines().count() < 80,
        "compact Markdown must not dump entire pages"
    );
}

#[test]
#[cfg(feature = "integration-tests")]
fn cli_search_with_output_file() -> anyhow::Result<()> {
    let (_temp_file, output_path) = temp_name("cli_test_", ".json")?;
    let output_str = output_path.to_string_lossy();

    cargo_bin_cmd!("bun-docs")
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

    cargo_bin_cmd!("bun-docs")
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

    cargo_bin_cmd!("bun-docs")
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

    cargo_bin_cmd!("bun-docs")
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

    cargo_bin_cmd!("bun-docs")
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
    cargo_bin_cmd!("bun-docs")
        .env("RUST_LOG", "debug")
        .args(["--search", "test"])
        .assert()
        .success()
        .stderr(predicate::str::contains("bun_docs"));
}

#[test]
#[cfg(feature = "integration-tests")]
fn cli_search_special_characters() {
    cargo_bin_cmd!("bun-docs")
        .args(["--search", "Bun.serve()"])
        .assert()
        .success();
}

#[test]
#[cfg(feature = "integration-tests")]
fn cli_search_relative_output_path() -> anyhow::Result<()> {
    let (_temp_file, output_path) = temp_name("cli_relative_", ".json")?;
    let output_str = output_path.to_string_lossy();

    cargo_bin_cmd!("bun-docs")
        .args(["--search", "test", "--output", output_str.as_ref()])
        .assert()
        .success();

    anyhow::ensure!(output_path.exists(), "output file missing");
    Ok(())
}

#[test]
#[cfg(feature = "integration-tests")]
fn cli_search_not_mcp_mode() {
    cargo_bin_cmd!("bun-docs")
        .args(["--search", "test"])
        .write_stdin("invalid json input\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"title\""));
}
