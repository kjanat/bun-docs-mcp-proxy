# Security Policy

## Supported Versions

We release security updates for the following versions:

| Version | Supported          |
| ------- | ------------------ |
| 0.2.x   | :white_check_mark: |
| < 0.2.0 | :x:                |

## Reporting a Vulnerability

We take security vulnerabilities seriously. If you discover a security issue, please follow these steps:

### 1. **Do Not** Open a Public Issue

Please **do not** create a public GitHub issue for security vulnerabilities. This could put users at risk.

### 2. Report Privately

Send details to: **info@kajkowalski.nl**

Include in your report:

- **Description** - Clear explanation of the vulnerability
- **Impact** - Who/what is affected
- **Reproduction** - Steps to reproduce the issue
- **Proof of Concept** - Code/commands demonstrating the issue (if applicable)
- **Suggested Fix** - If you have ideas for a fix (optional)
- **Your Contact Info** - So we can follow up with questions

### 3. What to Expect

- **Acknowledgment** - We'll confirm receipt within 48 hours
- **Initial Assessment** - We'll provide an initial assessment within 5 business days
- **Updates** - We'll keep you informed of progress
- **Credit** - With your permission, we'll credit you in the security advisory
- **Timeline** - We aim to release fixes within 30 days for critical issues

### 4. Disclosure Timeline

We follow coordinated disclosure:

1. **Day 0** - You report the vulnerability privately
2. **Day 1-5** - We assess and verify the issue
3. **Day 6-30** - We develop and test a fix
4. **Day 30** - We release the fix and publish a security advisory
5. **After release** - We may publish details after users have had time to update

We may request a longer disclosure period for complex issues.

## Security Best Practices

When using bun-docs-mcp-proxy:

### For Users

- **Keep Updated** - Always use the latest version
- **Verify Binaries** - Check release signatures/hashes
- **Secure Configuration** - Don't expose stdin/stdout to untrusted sources
- **Review Logs** - Monitor stderr logs for unusual activity
- **Network Security** - Proxy communicates with `bun.com/docs/mcp` over HTTPS

### For Developers

- **Dependency Updates** - We use Dependabot to track dependency vulnerabilities
- **Code Scanning** - We run clippy with strict lints and CI checks
- **Input Validation** - All JSON-RPC inputs are validated
- **Error Handling** - Errors are handled gracefully without exposing sensitive info
- **TLS** - We use `rustls-tls` for secure HTTPS communication

## Security Features

- **No Arbitrary Code Execution** - Proxy only forwards predefined JSON-RPC methods
- **Input Validation** - All inputs validated against JSON-RPC 2.0 schema
- **Secure Transport** - HTTPS-only communication with Bun Docs API
- **No Data Storage** - Proxy doesn't persist any data
- **Minimal Dependencies** - Small attack surface with carefully vetted dependencies
- **Memory Safety** - Written in Rust for memory safety guarantees

## Known Limitations

- **No Authentication** - Proxy itself doesn't authenticate clients (relies on MCP client for this)
- **Stdio Transport** - Security depends on the client's stdin/stdout security
- **Network Access** - Proxy requires network access to `bun.com/docs/mcp`

## Security Updates

Security fixes are released as:

- **Patch versions** - For backward-compatible security fixes (e.g., 0.2.1 → 0.2.2)
- **GitHub Security Advisories** - Published for all security issues
- **Release Notes** - Security fixes are clearly marked

Subscribe to releases on GitHub to get notified of security updates.

## Hall of Fame

We appreciate security researchers who help keep our users safe:

<!-- Security researchers who responsibly disclose vulnerabilities will be listed here -->

_No security issues reported yet._

## Questions?

For general security questions (not vulnerability reports):

- **GitHub Discussions** - https://github.com/kjanat/bun-docs-mcp-proxy/discussions
- **Email** - info@kajkowalski.nl

Thank you for helping keep bun-docs-mcp-proxy secure!
