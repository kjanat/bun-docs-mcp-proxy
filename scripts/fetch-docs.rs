#!/usr/bin/env rust-script
//! ```cargo
//! [dependencies]
//! reqwest = "0.12.24"
//! tokio = { version = "1.48.0", features = ["full"] }
//! ```

use tokio::fs;

const URL: &str = "https://bun.com/docs/runtime/http/websockets";

async fn fetch_with_headers(
    accept_header: Option<&str>,
    filename: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let header_name = accept_header.unwrap_or("default (HTML)");
    println!("Fetching {}...", header_name);

    let client = reqwest::Client::new();
    let mut request = client.get(URL);

    if let Some(header) = accept_header {
        request = request.header("Accept", header);
    }

    let response = request.send().await?;

    if !response.status().is_success() {
        eprintln!("Failed to fetch {}: {}", filename, response.status());
        return Ok(());
    }

    let content = response.text().await?;
    fs::write(filename, &content).await?;
    println!("✓ Written to {} ({} bytes)", filename, content.len());

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Fetch with no special header (HTML)
    fetch_with_headers(None, "websockets.raw").await?;

    // Fetch with text/markdown
    fetch_with_headers(Some("text/markdown"), "websockets.md").await?;

    // Fetch with text/plain
    fetch_with_headers(Some("text/plain"), "websockets.txt").await?;

    println!("\nDone!");
    Ok(())
}
