//! Validates the supported MCP messages against the published 2026-07-28 schema.
#![allow(clippy::tests_outside_test_module, reason = "integration test file")]

use assert_cmd::cargo::cargo_bin_cmd;
use bun_docs::{
    JsonRpcRequest, JsonRpcResponse,
    app::handlers::{handle_discover, handle_tools_call, handle_tools_list},
    docs::DocsCorpus,
};
use serde_json::{Value, json};

fn validate(definition: &str, instance: &Value) -> anyhow::Result<()> {
    let mut schema: Value =
        serde_json::from_str(include_str!("fixtures/mcp-2026-07-28.schema.json"))?;
    schema
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("schema must be an object"))?
        .insert("$ref".to_owned(), json!(format!("#/$defs/{definition}")));
    let validator = jsonschema::validator_for(&schema)?;
    validator
        .validate(instance)
        .map_err(|error| anyhow::anyhow!("{definition}: {error}"))
}

fn request(method: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": "schema-test",
        "method": method,
        "params": {
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientCapabilities": {}
            }
        }
    })
}

fn validate_response(response: JsonRpcResponse, definition: &str) -> anyhow::Result<()> {
    let serialized = serde_json::to_value(response)?;
    validate("JSONRPCResultResponse", &serialized)?;
    anyhow::ensure!(
        serialized.pointer("/result/_meta/io.modelcontextprotocol~1serverInfo/name")
            == Some(&json!("bun-docs")),
        "missing server identity"
    );
    validate(
        definition,
        serialized
            .get("result")
            .ok_or_else(|| anyhow::anyhow!("missing result"))?,
    )
}

#[test]
fn stdio_results_and_version_errors_match_published_schema() -> anyhow::Result<()> {
    let mut unsupported = request("tools/list");
    let version = unsupported
        .pointer_mut("/params/_meta/io.modelcontextprotocol~1protocolVersion")
        .ok_or_else(|| anyhow::anyhow!("missing protocol version"))?;
    *version = json!("1900-01-01");
    let inputs = [
        request("server/discover"),
        request("tools/list"),
        unsupported,
    ];
    let mut input = inputs
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    input.push('\n');
    let assertion = cargo_bin_cmd!("bun-docs")
        .write_stdin(input)
        .assert()
        .success();
    let output = String::from_utf8(assertion.get_output().stdout.clone())?;
    let responses = output
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?;
    anyhow::ensure!(responses.len() == 3, "expected three responses");
    for (response, definition) in responses.iter().zip([
        "DiscoverResult",
        "ListToolsResult",
        "UnsupportedProtocolVersionError",
    ]) {
        if definition == "UnsupportedProtocolVersionError" {
            validate("JSONRPCErrorResponse", response)?;
            validate(definition, response)?;
        } else {
            validate("JSONRPCResultResponse", response)?;
            validate(
                definition,
                response
                    .get("result")
                    .ok_or_else(|| anyhow::anyhow!("missing result"))?,
            )?;
        }
    }
    Ok(())
}

#[test]
fn tool_arguments_are_checked_against_the_advertised_schema() -> anyhow::Result<()> {
    let corpus = DocsCorpus::from_sources(
        "",
        "# Server\nSource: https://bun.com/docs/server\n\nUse Bun.serve.\n",
    )?;
    let listed = handle_tools_list(json!("schema-test"));
    let schema = listed
        .result
        .as_ref()
        .and_then(|result| result.pointer("/tools/0/inputSchema"))
        .ok_or_else(|| anyhow::anyhow!("missing input schema"))?;
    let validator = jsonschema::validator_for(schema)?;
    for arguments in [
        json!([]),
        Value::Null,
        json!({"query":1_i32}),
        json!({"query":"server", "limit":0_i32}),
        json!({"query":"server", "limit":26_i32}),
        json!({"query":"server", "extra":true}),
    ] {
        anyhow::ensure!(
            !validator.is_valid(&arguments),
            "fixture must violate the advertised schema"
        );
        let mut input = request("tools/call");
        let params = input
            .get_mut("params")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| anyhow::anyhow!("missing params"))?;
        params.insert("name".to_owned(), json!("search_bun"));
        params.insert("arguments".to_owned(), arguments);
        let parsed: JsonRpcRequest = serde_json::from_value(input)?;
        let response =
            serde_json::to_value(handle_tools_call(&corpus, &parsed, json!("schema-test")))?;
        validate("JSONRPCErrorResponse", &response)?;
        anyhow::ensure!(response.pointer("/error/code") == Some(&json!(-32_602_i32)));
    }
    Ok(())
}

#[test]
fn invalid_cursors_and_unissued_continuations_are_rejected() -> anyhow::Result<()> {
    for (method, field, value) in [
        ("tools/list", "cursor", json!("invalid")),
        ("server/discover", "requestState", json!("unissued")),
        ("tools/list", "inputResponses", json!({})),
    ] {
        let mut input = request(method);
        input
            .get_mut("params")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| anyhow::anyhow!("missing params"))?
            .insert(field.to_owned(), value);
        let assertion = cargo_bin_cmd!("bun-docs")
            .write_stdin(format!("{input}\n"))
            .assert()
            .success();
        let response: Value = serde_json::from_slice(&assertion.get_output().stdout)?;
        validate("JSONRPCErrorResponse", &response)?;
        anyhow::ensure!(response.pointer("/error/code") == Some(&json!(-32_602_i32)));
    }
    Ok(())
}

#[test]
fn discovery_matches_published_schema() -> anyhow::Result<()> {
    validate("DiscoverRequest", &request("server/discover"))?;
    validate_response(handle_discover(json!("schema-test")), "DiscoverResult")
}

#[test]
fn tool_list_matches_published_schema() -> anyhow::Result<()> {
    validate("ListToolsRequest", &request("tools/list"))?;
    validate_response(handle_tools_list(json!("schema-test")), "ListToolsResult")
}

#[test]
fn tool_call_matches_published_schema() -> anyhow::Result<()> {
    let corpus = DocsCorpus::from_sources(
        "- [Server](https://bun.com/docs/server.md): HTTP server.\n",
        "# Server\nSource: https://bun.com/docs/server\n\nUse Bun.serve.\n",
    )?;
    let mut input = request("tools/call");
    let params = input
        .get_mut("params")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| anyhow::anyhow!("missing params"))?;
    params.insert("name".to_owned(), json!("search_bun"));
    params.insert("arguments".to_owned(), json!({"query":"server"}));
    validate("CallToolRequest", &input)?;
    let parsed: JsonRpcRequest = serde_json::from_value(input)?;
    validate_response(
        handle_tools_call(&corpus, &parsed, json!("schema-test")),
        "CallToolResult",
    )
}

#[test]
fn unidentified_errors_match_published_schema() -> anyhow::Result<()> {
    let response = JsonRpcResponse::error(Value::Null, -32_700_i32, "Parse error".to_owned());
    validate("JSONRPCErrorResponse", &serde_json::to_value(response)?)
}

#[test]
fn malformed_messages_are_classified_and_schema_valid() -> anyhow::Result<()> {
    let assertion = cargo_bin_cmd!("bun-docs")
        .write_stdin("{\n{}\n[]\n{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":4}\n")
        .assert()
        .success();
    let output = String::from_utf8(assertion.get_output().stdout.clone())?;
    let responses = output
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?;
    anyhow::ensure!(
        responses.len() == 4,
        "expected one error per malformed message"
    );
    for (response, code) in
        responses
            .iter()
            .zip([-32_700_i32, -32_600_i32, -32_600_i32, -32_600_i32])
    {
        validate("JSONRPCErrorResponse", response)?;
        anyhow::ensure!(
            response.pointer("/error/code") == Some(&json!(code)),
            "wrong error classification: {response}"
        );
    }
    Ok(())
}
