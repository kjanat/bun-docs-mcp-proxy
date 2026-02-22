//! CLI integration tests for the bun-docs-mcp-proxy binary.
#![allow(clippy::tests_outside_test_module, reason = "integration test file")]

use core::time::Duration;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

#[test]
fn stdin_parse_error() {
    // Test that malformed JSON triggers parse error with proper error response.
    // Uses cargo_bin_cmd! which relies on CARGO_BIN_EXE_* (set by cargo for
    // integration tests) — no escargot fallback, works under cargo-llvm-cov.
    let mut cmd = cargo_bin_cmd!("bun-docs-mcp-proxy");
    cmd.write_stdin("{ invalid json without closing\n")
        .timeout(Duration::from_secs(2_u64))
        .assert()
        .stderr(
            predicate::str::contains("parse")
                .or(predicate::str::contains("Parse error"))
                .or(predicate::str::contains("EOF")),
        );
    // Verifies error logging in run_mcp_server parse error path
}
