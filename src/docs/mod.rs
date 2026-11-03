//! Static Bun documentation source.
//!
//! Sources documentation from Bun's published static assets rather than the
//! `bun.com/docs/mcp` endpoint:
//!
//! - `llms.txt` enumerates every page as `- [Title](url): description`.
//! - `llms-full.txt` concatenates every page body, each introduced by a
//!   `# Title` heading followed by a `Source: <url>` line.
//! - Each page is also served as Markdown at its `.md` URL.

use anyhow::{Context as _, Result};
use serde::Serialize;
use tantivy::{
    Index, IndexReader, TantivyDocument,
    collector::TopDocs,
    doc,
    query::QueryParser,
    schema::{Field, IndexRecordOption, STORED, Schema, TextFieldIndexing, TextOptions, Value},
    snippet::SnippetGenerator,
};

/// Bun's machine-readable documentation index.
pub const BUN_DOCS_INDEX_URL: &str = "https://bun.com/llms.txt";

/// Bun's complete documentation corpus.
pub const BUN_DOCS_FULL_URL: &str = "https://bun.com/llms-full.txt";

/// Number of results returned by the CLI search.
pub const DEFAULT_SEARCH_LIMIT: usize = 10;

/// One entry from the `llms.txt` index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocEntry {
    /// Human-readable page title.
    pub title: String,
    /// Absolute `.md` URL of the page.
    pub url: String,
    /// One-line page description, empty when the index omits it.
    pub description: String,
}

/// One page body extracted from `llms-full.txt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocPage {
    /// Page title from the `# Title` heading.
    pub title: String,
    /// Absolute URL from the page's `Source:` line.
    pub url: String,
    /// Full Markdown body following the `Source:` line.
    pub body: String,
}

/// Parses the `llms.txt` index into its entries.
///
/// Recognises Markdown list items of the form `- [Title](url): description`;
/// the `: description` suffix is optional. Non-list lines (headings, blanks)
/// are ignored.
#[must_use]
pub fn parse_index(text: &str) -> Vec<DocEntry> {
    text.lines().filter_map(parse_index_line).collect()
}

fn parse_index_line(line: &str) -> Option<DocEntry> {
    let after_bullet = line.trim().strip_prefix("- [")?;
    let (title, after_title) = after_bullet.split_once("](")?;
    let (url, tail) = after_title.split_once(')')?;
    let description = tail.strip_prefix(':').map_or("", str::trim);
    Some(DocEntry {
        title: title.to_owned(),
        url: url.to_owned(),
        description: description.to_owned(),
    })
}

/// Splits `llms-full.txt` into its constituent pages.
///
/// A page begins at a `# Title` heading immediately followed by a
/// `Source: <url>` line, and runs until the next such boundary. A bare `# `
/// line inside a fenced code block is not a boundary because it is not
/// followed by a `Source:` line.
#[must_use]
pub fn parse_full(text: &str) -> Vec<DocPage> {
    let lines: Vec<&str> = text.lines().collect();
    let boundaries: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|&(i, _)| is_page_boundary(&lines, i))
        .map(|(i, _)| i)
        .collect();

    boundaries
        .iter()
        .enumerate()
        .filter_map(|(nth, &start)| {
            let title = lines.get(start)?.strip_prefix("# ")?.trim().to_owned();
            let url = lines
                .get(start + 1)?
                .strip_prefix("Source:")?
                .trim()
                .to_owned();
            let end = boundaries.get(nth + 1).copied().unwrap_or(lines.len());
            let body = lines.get(start + 2..end)?.join("\n").trim().to_owned();
            Some(DocPage { title, url, body })
        })
        .collect()
}

fn is_page_boundary(lines: &[&str], index: usize) -> bool {
    lines.get(index).is_some_and(|line| line.starts_with("# "))
        && lines
            .get(index + 1)
            .is_some_and(|line| line.starts_with("Source:"))
}

/// A ranked search result.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SearchHit {
    /// Page title.
    pub title: String,
    /// Absolute page URL.
    pub url: String,
    /// Curated one-line description, empty when the index has none.
    pub description: String,
    /// Relevance score; higher is more relevant.
    pub score: f32,
}

/// Downloaded Bun documentation and its in-memory search index.
pub struct DocsCorpus {
    pages: Vec<DocPage>,
    index: DocsIndex,
}

impl DocsCorpus {
    /// Downloads Bun's static documentation sources and builds the local index.
    ///
    /// # Errors
    ///
    /// Returns an error when either source cannot be downloaded or indexed.
    pub async fn fetch() -> Result<Self> {
        let client = reqwest::Client::new();
        let (index_text, full_text) = tokio::try_join!(
            fetch_text(&client, BUN_DOCS_INDEX_URL),
            fetch_text(&client, BUN_DOCS_FULL_URL),
        )?;
        Self::from_sources(&index_text, &full_text)
    }

    /// Builds a corpus from already-loaded static sources.
    ///
    /// # Errors
    ///
    /// Returns an error when the full corpus contains no pages or indexing fails.
    pub fn from_sources(index_text: &str, full_text: &str) -> Result<Self> {
        let entries = parse_index(index_text);
        let pages = parse_full(full_text);
        anyhow::ensure!(
            !pages.is_empty(),
            "Bun documentation corpus contains no pages"
        );
        let index = DocsIndex::build(&pages, &entries)?;
        Ok(Self { pages, index })
    }

    /// Searches the local documentation index.
    ///
    /// # Errors
    ///
    /// Returns an error if the query cannot be parsed or the search fails.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        self.index.search(query, limit)
    }

    /// Returns the full page matching a search result URL.
    #[must_use]
    pub fn page(&self, url: &str) -> Option<&DocPage> {
        self.pages
            .iter()
            .find(|page| normalize_url(&page.url) == normalize_url(url))
    }

    /// Extracts a short passage matching the query from a page's Markdown body.
    ///
    /// # Errors
    ///
    /// Returns an error if the query's snippet generator cannot be created.
    pub fn excerpt(&self, query: &str, url: &str) -> Result<String> {
        let Some(page) = self.page(url) else {
            return Ok(String::new());
        };
        let (parsed, _) = self.index.query_parser.parse_query_lenient(query);
        let mut generator = SnippetGenerator::create(
            &self.index.reader.searcher(),
            &*parsed,
            self.index.field_body,
        )?;
        generator.set_max_num_chars(240);
        let prose = excerpt_prose(&page.body);
        let snippet = generator.snippet(&prose);
        let fragment = if snippet.fragment().is_empty() {
            prose.as_str()
        } else {
            snippet.fragment()
        };
        let normalized = fragment.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut excerpt: String = normalized.chars().take(240).collect();
        if normalized.chars().count() > 240
            && let Some(boundary) = excerpt.rfind(char::is_whitespace)
        {
            excerpt.truncate(boundary);
        }
        if normalized.chars().count() > 240 || !prose.trim_end().ends_with(fragment.trim_end()) {
            excerpt.push_str("...");
        }
        Ok(excerpt)
    }
}

fn excerpt_prose(markdown: &str) -> String {
    let mut fence = None;
    markdown
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                let marker = trimmed.chars().next()?;
                if fence == Some(marker) {
                    fence = None;
                } else if fence.is_none() {
                    fence = Some(marker);
                }
                return None;
            }
            if fence.is_some()
                || trimmed.starts_with(['#', '<', '|'])
                || matches!(trimmed, "***" | "---" | "___")
            {
                return None;
            }
            Some(trimmed)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

async fn fetch_text(client: &reqwest::Client, url: &str) -> Result<String> {
    client
        .get(url)
        .send()
        .await
        .with_context(|| format!("failed to download {url}"))?
        .error_for_status()
        .with_context(|| format!("Bun documentation source returned an error: {url}"))?
        .text()
        .await
        .with_context(|| format!("failed to read {url}"))
}

/// In-memory full-text index over the documentation pages.
///
/// Title matches are boosted over body matches so an exact-topic query surfaces
/// its page ahead of pages that merely mention the term.
pub struct DocsIndex {
    reader: IndexReader,
    query_parser: QueryParser,
    field_title: Field,
    field_url: Field,
    field_description: Field,
    field_body: Field,
}

impl DocsIndex {
    /// Builds an in-RAM index from the pages, enriched with the index entries.
    ///
    /// Each page is matched to its `llms.txt` entry by URL (ignoring a trailing
    /// `.md`) so the curated description is indexed and returned with hits.
    ///
    /// # Errors
    ///
    /// Returns an error if the index writer cannot be created or a document
    /// fails to commit.
    pub fn build(pages: &[DocPage], entries: &[DocEntry]) -> Result<Self> {
        let mut schema_builder = Schema::builder();
        let stemmed = TextFieldIndexing::default()
            .set_tokenizer("en_stem")
            .set_index_option(IndexRecordOption::WithFreqsAndPositions);
        let stemmed_stored = TextOptions::default()
            .set_indexing_options(stemmed.clone())
            .set_stored();
        let field_title = schema_builder.add_text_field("title", stemmed_stored.clone());
        let field_description = schema_builder.add_text_field("description", stemmed_stored);
        let field_url = schema_builder.add_text_field("url", STORED);
        let field_body = schema_builder
            .add_text_field("body", TextOptions::default().set_indexing_options(stemmed));
        let schema = schema_builder.build();

        let index = Index::create_in_ram(schema);
        let mut writer = index.writer(50_000_000)?;
        for page in pages {
            let description = entries
                .iter()
                .find(|entry| normalize_url(&entry.url) == normalize_url(&page.url))
                .map_or("", |entry| entry.description.as_str());
            writer.add_document(doc!(
                field_title => page.title.clone(),
                field_description => description.to_owned(),
                field_url => page.url.clone(),
                field_body => page.body.clone(),
            ))?;
        }
        writer.commit()?;

        let reader = index.reader()?;
        let mut query_parser =
            QueryParser::for_index(&index, vec![field_title, field_description, field_body]);
        query_parser.set_field_boost(field_title, 3.0);
        query_parser.set_field_boost(field_description, 2.0);

        Ok(Self {
            reader,
            query_parser,
            field_title,
            field_url,
            field_description,
            field_body,
        })
    }

    /// Returns up to `limit` pages ranked by relevance to `query`.
    ///
    /// # Errors
    ///
    /// Returns an error if the query cannot be parsed or the search fails.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let searcher = self.reader.searcher();
        let (parsed, _) = self.query_parser.parse_query_lenient(query);
        let top_docs = searcher.search(&parsed, &TopDocs::with_limit(limit).order_by_score())?;

        let mut hits = Vec::with_capacity(top_docs.len());
        for (score, address) in top_docs {
            let document: TantivyDocument = searcher.doc(address)?;
            hits.push(SearchHit {
                title: field_str(&document, self.field_title),
                url: field_str(&document, self.field_url),
                description: field_str(&document, self.field_description),
                score,
            });
        }
        Ok(hits)
    }
}

fn normalize_url(url: &str) -> &str {
    url.strip_suffix(".md").unwrap_or(url)
}

fn field_str(document: &TantivyDocument, field: Field) -> String {
    document
        .get_first(field)
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_owned()
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::assert_is_empty,
        clippy::expect_used,
        clippy::indexing_slicing,
        reason = "test code"
    )]

    use super::*;

    #[test]
    fn excerpts_skip_code_fences_and_component_markup() {
        let body = "# Server\nSource: https://bun.com/docs/server\n\nUse Bun.serve to start an HTTP server.\n\n<CodeGroup>\n```ts theme={private-looking-metadata}\nserver server server server server\n```\n</CodeGroup>\n";
        let corpus = DocsCorpus::from_sources("", body).expect("corpus builds");
        let excerpt = corpus
            .excerpt("server", "https://bun.com/docs/server")
            .expect("excerpt generated");
        assert!(excerpt.contains("Use Bun.serve"));
        assert!(!excerpt.contains("theme="));
        assert!(!excerpt.contains("CodeGroup"));
        assert!(!excerpt.contains("server server"));
    }

    #[test]
    fn excerpt_finds_query_beyond_intro_and_bounds_unicode_output() {
        let body = format!(
            "{}\n\nWebSockets send café updates to connected clients. {}",
            "Unrelated introduction. ".repeat(40),
            "日本語 ".repeat(200)
        );
        let full = format!("# Server\nSource: https://bun.com/docs/server\n\n{body}\n");
        let corpus = DocsCorpus::from_sources("", &full).expect("corpus builds");
        let excerpt = corpus
            .excerpt("websocket", "https://bun.com/docs/server")
            .expect("excerpt generated");
        assert!(excerpt.contains("WebSockets"));
        assert!(excerpt.chars().count() <= 243);
        assert!(!excerpt.contains('\n'));
        assert!(excerpt.ends_with("..."));
    }

    #[test]
    fn excerpt_falls_back_for_title_only_matches() {
        let corpus = DocsCorpus::from_sources("", "# Server\nSource: https://bun.com/docs/server\n\nUse Bun.serve to listen for requests.\n").expect("corpus builds");
        assert_eq!(
            corpus
                .excerpt("title:server", "https://bun.com/docs/server")
                .expect("excerpt generated"),
            "Use Bun.serve to listen for requests."
        );
        assert_eq!(
            corpus
                .excerpt("server", "https://bun.com/docs/missing")
                .expect("missing page"),
            ""
        );
    }

    #[test]
    fn parse_index_reads_title_url_and_description() {
        let text = "\
# Bun

## Docs

- [Welcome to Bun](https://bun.com/docs/index.md): Bun is an all-in-one toolkit.
- [Installation](https://bun.com/docs/installation.md): Install Bun with npm.
";
        let entries = parse_index(text);
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0],
            DocEntry {
                title: "Welcome to Bun".to_owned(),
                url: "https://bun.com/docs/index.md".to_owned(),
                description: "Bun is an all-in-one toolkit.".to_owned(),
            }
        );
        assert_eq!(entries[1].title, "Installation");
        assert_eq!(entries[1].url, "https://bun.com/docs/installation.md");
    }

    #[test]
    fn parse_index_tolerates_missing_description() {
        let text = "- [Quickstart](https://bun.com/docs/quickstart.md)\n";
        let entries = parse_index(text);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Quickstart");
        assert_eq!(entries[0].url, "https://bun.com/docs/quickstart.md");
        assert_eq!(entries[0].description, "");
    }

    #[test]
    fn parse_index_ignores_non_list_lines() {
        let text = "# Bun\n\n## Docs\n\nsome prose\n";
        assert!(parse_index(text).is_empty());
    }

    #[test]
    fn parse_full_splits_pages_on_heading_and_source() {
        let text = "\
# Bytecode Caching
Source: https://bun.com/docs/bundler/bytecode

Speed up execution with bytecode caching.

## Usage

Enable it with the flag.

# Installation
Source: https://bun.com/docs/installation

Install Bun with npm.
";
        let pages = parse_full(text);
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].title, "Bytecode Caching");
        assert_eq!(pages[0].url, "https://bun.com/docs/bundler/bytecode");
        assert!(pages[0].body.contains("Speed up execution"));
        assert!(pages[0].body.contains("## Usage"));
        assert!(!pages[0].body.contains("# Installation"));
        assert_eq!(pages[1].title, "Installation");
        assert_eq!(pages[1].url, "https://bun.com/docs/installation");
        assert!(pages[1].body.contains("Install Bun with npm."));
    }

    fn sample_pages() -> Vec<DocPage> {
        vec![
            DocPage {
                title: "WebSockets".to_owned(),
                url: "https://bun.com/docs/api/websockets".to_owned(),
                body: "Server-side WebSockets in Bun.".to_owned(),
            },
            DocPage {
                title: "HTTP Server".to_owned(),
                url: "https://bun.com/docs/api/http".to_owned(),
                body: "Use Bun.serve to start a server; supports websocket upgrades.".to_owned(),
            },
        ]
    }

    #[test]
    fn search_ranks_title_match_above_body_mention() {
        let index = DocsIndex::build(&sample_pages(), &[]).expect("index builds");
        let hits = index.search("websockets", 5).expect("search runs");
        assert!(!hits.is_empty());
        assert_eq!(hits[0].url, "https://bun.com/docs/api/websockets");
        assert_eq!(hits[0].title, "WebSockets");
    }

    #[test]
    fn search_matches_singular_query_to_plural_title_via_stemming() {
        let pages = vec![
            DocPage {
                title: "Cookies".to_owned(),
                url: "https://bun.com/docs/api/cookies".to_owned(),
                body: "Work with cookies in requests and responses.".to_owned(),
            },
            DocPage {
                title: "Fetch".to_owned(),
                url: "https://bun.com/docs/api/fetch".to_owned(),
                body: "Send HTTP requests with the fetch API.".to_owned(),
            },
        ];
        let index = DocsIndex::build(&pages, &[]).expect("index builds");
        let hits = index.search("cookie", 5).expect("search runs");
        assert!(
            !hits.is_empty(),
            "singular query should match the plural title"
        );
        assert_eq!(hits[0].url, "https://bun.com/docs/api/cookies");
    }

    #[test]
    fn description_is_indexed_and_returned_in_hits() {
        let pages = vec![DocPage {
            title: "Server".to_owned(),
            url: "https://bun.com/docs/api/http".to_owned(),
            body: "Start an HTTP server.".to_owned(),
        }];
        let entries = vec![DocEntry {
            title: "Server".to_owned(),
            url: "https://bun.com/docs/api/http.md".to_owned(),
            description: "A blazing-fast websocket-capable listener.".to_owned(),
        }];
        let index = DocsIndex::build(&pages, &entries).expect("index builds");

        let hits = index.search("listener", 5).expect("search runs");
        assert!(
            !hits.is_empty(),
            "a term only in the description must be searchable"
        );
        assert_eq!(hits[0].url, "https://bun.com/docs/api/http");
        assert_eq!(
            hits[0].description,
            "A blazing-fast websocket-capable listener."
        );
    }

    #[test]
    fn search_respects_limit_and_misses_return_empty() {
        let index = DocsIndex::build(&sample_pages(), &[]).expect("index builds");
        assert_eq!(index.search("server", 1).expect("search runs").len(), 1);
        assert!(
            index
                .search("nonexistentterm", 5)
                .expect("search runs")
                .is_empty()
        );
    }

    #[test]
    fn corpus_search_and_page_lookup_use_local_sources() {
        let index_text = "- [HTTP Server](https://bun.com/docs/api/http.md): Serve requests.\n";
        let full_text = "\
# HTTP Server
Source: https://bun.com/docs/api/http

Use Bun.serve to start a server.
";
        let corpus = DocsCorpus::from_sources(index_text, full_text).expect("corpus builds");

        let hits = corpus.search("server", 1).expect("search runs");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].description, "Serve requests.");
        assert_eq!(
            corpus.page(&hits[0].url).map(|page| page.body.as_str()),
            Some("Use Bun.serve to start a server.")
        );
    }

    #[test]
    fn corpus_rejects_empty_full_source() {
        let error = DocsCorpus::from_sources("", "").err().expect("must fail");
        assert!(error.to_string().contains("contains no pages"));
    }

    #[test]
    fn parse_full_ignores_hash_lines_inside_code_blocks() {
        let text = "\
# Real Page
Source: https://bun.com/docs/real

```bash
# ESM (requires --compile)
bun build ./cli.ts
```
";
        let pages = parse_full(text);
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].title, "Real Page");
        assert!(pages[0].body.contains("# ESM (requires --compile)"));
    }
}
