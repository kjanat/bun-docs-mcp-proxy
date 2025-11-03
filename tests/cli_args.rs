use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::process::Command;

#[test]
fn unknown_arg_exits_nonzero() {
    // NOTE: Tests handle_args unknown argument path that calls exit(1)
    // This covers the previously untested error branch in main.rs:84-87
    let mut cmd = Command::cargo_bin("bun-docs-mcp-proxy").unwrap();
    cmd.arg("--invalid-flag-xyz")
        .assert()
        .failure() // exit code should be non-zero
        .stderr(
            predicate::str::contains("Unknown argument").or(predicate::str::contains("unknown")),
        );
}

#[test]
fn help_flag_exits_success() {
    let mut cmd = Command::cargo_bin("bun-docs-mcp-proxy").unwrap();
    cmd.arg("--help").assert().success().stdout(
        predicate::str::contains("USAGE").or(predicate::str::contains("bun-docs-mcp-proxy")),
    );
}

#[test]
fn help_short_flag_exits_success() {
    let mut cmd = Command::cargo_bin("bun-docs-mcp-proxy").unwrap();
    cmd.arg("-h")
        .assert()
        .success()
        .stdout(predicate::str::contains("USAGE").or(predicate::str::contains("FLAGS")));
}

#[test]
fn version_flag_exits_success() {
    let mut cmd = Command::cargo_bin("bun-docs-mcp-proxy").unwrap();
    cmd.arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"bun-docs-mcp-proxy \d+\.\d+\.\d+").unwrap());
}

#[test]
fn version_short_flag_exits_success() {
    let mut cmd = Command::cargo_bin("bun-docs-mcp-proxy").unwrap();
    cmd.arg("-V")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"\d+\.\d+\.\d+").unwrap());
}

#[test]
fn handles_stdin_eof_cleanly() {
    // NOTE: Tests read_message EOF handling (bytes_read == 0 in transport/mod.rs:24-27)
    // Empty stdin should trigger EOF and exit cleanly
    let mut cmd = Command::cargo_bin("bun-docs-mcp-proxy").unwrap();
    // No stdin written → immediate EOF
    cmd.assert().success(); // Should exit 0 on clean EOF
}

#[test]
fn handles_invalid_json_gracefully() {
    // NOTE: Tests JSON parse error handling in main event loop (main.rs:124-130)
    // Invalid JSON should log error and continue or exit gracefully
    let mut cmd = Command::cargo_bin("bun-docs-mcp-proxy").unwrap();
    cmd.write_stdin("not valid json at all\n")
        .timeout(std::time::Duration::from_secs(2))
        .assert()
        .stderr(
            predicate::str::contains("parse")
                .or(predicate::str::contains("JSON"))
                .or(predicate::str::is_empty()),
        );
    // May exit with error or continue - either is acceptable
}

#[test]
fn initialize_roundtrip() {
    // NOTE: Tests actual read_message/write_message roundtrip through real stdin/stdout
    // This covers transport/mod.rs async methods that can't be unit tested
    let mut cmd = Command::cargo_bin("bun-docs-mcp-proxy").unwrap();
    cmd.write_stdin(r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#)
        .write_stdin("\n")
        .timeout(std::time::Duration::from_secs(2))
        .assert()
        .success()
        .stdout(predicate::str::contains("protocolVersion"))
        .stdout(predicate::str::contains("2024-11-05"));
}
