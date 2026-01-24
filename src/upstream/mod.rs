//! Upstream API clients for external services.

pub mod bun_docs;

// Re-export client types (BunDocsClientBuilder/Config for potential future use)
#[allow(unused_imports, reason = "re-exports for public API consistency")]
pub use bun_docs::{BunDocsClient, BunDocsClientBuilder, BunDocsClientConfig, UpstreamResponse};
