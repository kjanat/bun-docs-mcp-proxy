#!/usr/bin/env bun

// Fetch Bun docs with different Accept headers
const url = "https://bun.com/docs/runtime/http/websockets";

async function fetchWithHeaders(acceptHeader: string | null, filename: string) {
  const headers: Record<string, string> = {};
  if (acceptHeader) {
    headers["Accept"] = acceptHeader;
  }

  console.log(`Fetching ${acceptHeader || "default (HTML)"}...`);

  try {
    const response = await fetch(url, { headers });

    if (!response.ok) {
      console.error(
        `Failed to fetch ${filename}: ${response.status} ${response.statusText}`,
      );
      return;
    }

    const content = await response.text();
    await Bun.write(filename, content);
    console.log(`✓ Written to ${filename} (${content.length} bytes)`);
  } catch (error) {
    console.error(`Error fetching ${filename}:`, error);
  }
}

// Fetch with no special header (HTML)
await fetchWithHeaders(null, "websockets.raw");

// Fetch with text/markdown
await fetchWithHeaders("text/markdown", "websockets.md");

// Fetch with text/plain
await fetchWithHeaders("text/plain", "websockets.txt");

console.log("\nDone!");
