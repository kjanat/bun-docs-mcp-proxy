use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

#[test]
fn unknown_arg_exits_nonzero() {
    let mut cmd = cargo_bin_cmd!("bun-docs-mcp-proxy");
    cmd.arg("--invalid-flag-xyz")
        .assert()
        .failure() // exit code should be non-zero
        .stderr(
            predicate::str::contains("unexpected argument")
                .or(predicate::str::contains("unknown"))
                .or(predicate::str::contains("unrecognized")),
        );
}

#[test]
fn help_flag_exits_success() {
    let mut cmd = cargo_bin_cmd!("bun-docs-mcp-proxy");
    cmd.arg("--help").assert().success().stdout(
        predicate::str::contains("USAGE").or(predicate::str::contains("bun-docs-mcp-proxy")),
    );
}

#[test]
fn help_short_flag_exits_success() {
    let mut cmd = cargo_bin_cmd!("bun-docs-mcp-proxy");
    cmd.arg("-h").assert().success().stdout(
        predicate::str::contains("Usage")
            .or(predicate::str::contains("Options"))
            .or(predicate::str::contains("search")),
    );
}

#[test]
fn version_flag_exits_success() {
    let mut cmd = cargo_bin_cmd!("bun-docs-mcp-proxy");
    cmd.arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"bun-docs-mcp-proxy \d+\.\d+\.\d+").unwrap());
}

#[test]
fn version_short_flag_exits_success() {
    let mut cmd = cargo_bin_cmd!("bun-docs-mcp-proxy");
    cmd.arg("-V")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"\d+\.\d+\.\d+").unwrap());
}

#[test]
fn handles_stdin_eof_cleanly() {
    let mut cmd = cargo_bin_cmd!("bun-docs-mcp-proxy");
    // No stdin written → immediate EOF
    cmd.assert().success(); // Should exit 0 on clean EOF
}

#[test]
fn handles_invalid_json_gracefully() {
    let mut cmd = cargo_bin_cmd!("bun-docs-mcp-proxy");
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
    let mut cmd = cargo_bin_cmd!("bun-docs-mcp-proxy");
    cmd.write_stdin(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}
"#,
    )
    .timeout(std::time::Duration::from_secs(2))
    .assert()
    .success()
    .stdout(predicate::str::contains("protocolVersion"))
    .stdout(predicate::str::contains("2024-11-05"));
}
