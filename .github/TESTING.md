# Testing Documentation

## Overview

This project uses a **dual testing strategy** that separates fast, network-free unit tests from integration tests that require real API calls to `bun.com/docs/mcp`. This ensures CI runs quickly and reliably while maintaining comprehensive test coverage.

## Test Types

### Unit Tests (Default)

- **Fast**: Run in 2-3 seconds
- **Reliable**: No network dependencies, no random failures
- **Mocked**: Use `mockito` to simulate API responses
- **Always Run**: Execute on every commit and PR

### Integration Tests (Opt-in)

- **Real API**: Make actual HTTP calls to `bun.com/docs/mcp`
- **Feature-gated**: Require `--features integration-tests` flag
- **Scheduled**: Run daily via GitHub Actions
- **Manual**: Can be triggered manually when needed

## Running Tests

### Using Task (Recommended)

```bash
# Run unit tests only (default, fast)
task test              # or task t

# Run only mocked unit tests
task test-unit-only    # or task tuo

# Run integration tests with real API
task test-integration-only  # or task tio

# Run ALL tests including integration
task test-with-integration  # or task twi
```

### Using Cargo Directly

```bash
# Run unit tests only (default)
cargo test

# Run integration tests only
cargo test --features integration-tests

# Run specific integration test
cargo test --features integration-tests test_handle_tools_call_real_api
```

## Test Organization

### File Structure

```
src/
├── main_tests.rs          # All unit and integration tests
│   ├── Unit tests         # Always run (65+ tests)
│   └── Integration tests  # Feature-gated (8 tests)
tests/
└── integration_test.rs    # Protocol compliance tests (14 tests)
```

### Integration Tests (Feature-Gated)

The following 8 tests in `src/main_tests.rs` require the `integration-tests` feature:

1. `test_handle_tools_call_real_api` - Tests real API search
2. `test_handle_tools_call_empty_query` - Tests empty query handling
3. `test_handle_resources_read_with_query` - Tests resource read with query
4. `test_handle_resources_read_empty_query` - Tests resource read without query
5. `test_handle_resources_read_with_real_search` - Tests real search via resources
6. `test_direct_search_json_format` - Tests CLI JSON output
7. `test_direct_search_text_format` - Tests CLI text output
8. `test_direct_search_markdown_format` - Tests CLI markdown output with MDX fetch

### Mocked Unit Tests

Two new mocked tests provide coverage without network calls:

1. `test_handle_tools_call_mocked` - Simulates successful API response
2. `test_handle_resources_read_mocked` - Simulates resource read

## CI/CD Integration

### Default CI (`ci.yml`)

- Runs on every push and PR
- Executes unit tests only (no network)
- Fast and reliable (~30 seconds total)

### Integration Tests CI (`integration-tests.yml`)

- Scheduled daily at 3 AM UTC
- Manual trigger via GitHub Actions UI
- Continues on error (network issues don't fail CI)
- Reports coverage separately

## Writing Tests

### Writing Unit Tests

```rust
#[tokio::test]
async fn test_my_feature_mocked() {
    // Use mockito to simulate API responses
    let mut server = mockito::Server::new_async().await;

    let mock = server
        .mock("POST", "/mcp")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body("data: {\"result\":{...}}\n\n")
        .create_async()
        .await;

    // Test with mocked client
    let client = BunDocsClient::with_base_url(&server.url()).unwrap();
    // ... test logic ...

    mock.assert_async().await;
}
```

### Writing Integration Tests

```rust
#[tokio::test]
#[cfg(feature = "integration-tests")]
async fn test_my_feature_real_api() {
    // Use real client
    let client = BunDocsClient::new();

    // Make real API call
    let response = client.search("query").await;

    // Verify real response
    assert!(response.is_ok());
}
```

## Best Practices

1. **Default to Unit Tests**: Write mocked tests for new features
2. **Integration for Validation**: Add integration tests for critical paths
3. **Handle Network Failures**: Integration tests should gracefully handle API issues
4. **Document API Behavior**: Note any quirks in Bun API responses
5. **Keep Tests Fast**: Optimize test performance for developer experience

## Troubleshooting

### Tests Failing Locally

```bash
# Check if it's an integration test issue
task test-unit-only  # Should pass
task test-integration-only  # May fail due to network

# Enable debug logging
RUST_LOG=debug cargo test
```

### Integration Tests Failing in CI

- Check the [Integration Tests workflow](https://github.com/kjanat/bun-docs-mcp-proxy/actions/workflows/integration-tests.yml)
- Network issues are expected occasionally
- API changes may require test updates

### Running Specific Tests

```bash
# Run single test
cargo test test_handle_initialize

# Run tests matching pattern
cargo test handle_tools

# Run with verbose output
cargo test -- --nocapture
```

## Coverage

### Generate Coverage Report

```bash
# Unit tests coverage
task coverage

# Integration tests coverage
cargo llvm-cov --features integration-tests

# HTML report
task coverage-html
```

## Migration Guide

If you had tests running before this change:

1. Unit tests continue to work as before
2. Integration tests now require `--features integration-tests`
3. CI automatically handles the right test types
4. No code changes needed for existing tests
