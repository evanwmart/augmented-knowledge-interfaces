// src/ingest.rs

//! Ingestion module for Basic RAG
//!
//! Responsibilities:
//! 1. Clone or pull the target Git repository, extracting only the documentation folder.
//! 2. Walk the local docs directory and split files into overlapping token-based chunks.
//! 3. Compute checksums per file and compare to previous state for incremental updates.

use anyhow::{Context, Result, anyhow};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use walkdir::WalkDir;
use sha2::{Sha256, Digest};
use serde::{Deserialize, Serialize};
use log::{info, debug, warn};
use crate::cli::Cli;

/// Represents a chunk of text with source metadata.
#[derive(Debug, Clone)]
pub struct Chunk {
    pub id: String,           // Unique chunk ID, e.g. "path/to/file.md:chunk3"
    pub text: String,         // The chunk's text content
    pub source: String,       // Source file path or URL fragment
    pub heading: Option<String>, // Optional heading extracted from the file
    pub position: usize,      // Chunk index within the file
}

/// State tracking for incremental updates
#[derive(Debug, Serialize, Deserialize, Default)]
struct IngestState {
    /// Maps file path to its SHA-256 checksum
    file_checksums: HashMap<String, String>,
}

impl IngestState {
    /// Load state from state.json file
    fn load() -> Result<Self> {
        let state_path = Path::new("state.json");
        if !state_path.exists() {
            return Ok(Self::default());
        }
        
        let contents = fs::read_to_string(state_path)
            .context("Failed to read state.json")?;
        
        let state: IngestState = serde_json::from_str(&contents)
            .context("Failed to parse state.json")?;
        
        Ok(state)
    }
    
    /// Save state to state.json file
    fn save(&self) -> Result<()> {
        let contents = serde_json::to_string_pretty(self)
            .context("Failed to serialize state")?;
        
        fs::write("state.json", contents)
            .context("Failed to write state.json")?;
        
        Ok(())
    }
}

/// Ensure the docs directory is populated by cloning or pulling the Git repo.
pub fn sync_docs(docs_dir: &Path) -> Result<()> {
    info!("🔄 Syncing docs to {:?}", docs_dir);
    
    // For now, just create the directory if it doesn't exist
    // TODO: This is a placeholder - in a real implementation, you'd add:
    // - Git repository cloning/pulling logic
    // - Sparse checkout functionality
    // - Error handling for Git operations
    std::fs::create_dir_all(docs_dir)
        .context("Failed to create docs directory")?;
    
    // Create some sample files for testing if the directory is empty
    if docs_dir.read_dir()?.next().is_none() {
        warn!("Docs directory is empty - creating sample files for testing");
        create_sample_docs(docs_dir)?;
    }
    
    Ok(())
}

/// Create sample documentation files for testing
fn create_sample_docs(docs_dir: &Path) -> Result<()> {
    let sample_content = r#"# Getting Started

This is a sample documentation file for testing the RAG system.

## Installation

To install the application, follow these steps:

1. Download the latest release
2. Extract the archive
3. Run the installer

## Configuration

The application can be configured using a configuration file:

```toml
[server]
host = "localhost"
port = 8080

[database]
url = "sqlite:///app.db"
```

## Usage

Basic usage examples:

- Start the server: `app start`
- Stop the server: `app stop`
- Check status: `app status`
"#;

    let guide_content = r#"# User Guide

Welcome to the comprehensive user guide.

## Features

### Core Features

- **Document Processing**: Advanced text processing capabilities
- **Search**: Full-text search with BM25 ranking
- **API Integration**: RESTful API for external integrations

### Advanced Features

- **Custom Plugins**: Extend functionality with custom plugins
- **Batch Processing**: Process multiple documents simultaneously
- **Real-time Updates**: Live document updates and indexing

## Troubleshooting

### Common Issues

**Issue: Application won't start**
- Check if port 8080 is available
- Verify configuration file syntax
- Check log files for errors

**Issue: Search returns no results**
- Rebuild the search index
- Check document permissions
- Verify query syntax

## Best Practices

1. Regular backups of important data
2. Monitor system resources
3. Keep documentation up to date
4. Use meaningful file names
"#;

    fs::write(docs_dir.join("getting-started.md"), sample_content)
        .context("Failed to create getting-started.md")?;
    
    fs::write(docs_dir.join("user-guide.md"), guide_content)
        .context("Failed to create user-guide.md")?;
    
    Ok(())
}

/// Walk the docs directory and chunk each file into token-based chunks.
pub fn ingest_docs(cli: &Cli) -> Result<Vec<Chunk>> {
    info!("📖 Ingesting and chunking docs in {:?}", cli.docs_dir);
    
    // Load previous state
    let mut state = IngestState::load()
        .context("Failed to load ingestion state")?;
    
    let mut all_chunks = Vec::new();
    let mut new_checksums = HashMap::new();
    
    // Walk the docs directory
    for entry in WalkDir::new(&cli.docs_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| is_supported_file(e.path()))
    {
        let file_path = entry.path();
        let relative_path = file_path.strip_prefix(&cli.docs_dir)
            .unwrap_or(file_path)
            .to_string_lossy()
            .to_string();
        
        debug!("Processing file: {:?}", file_path);
        
        // Read file content
        let content = match fs::read_to_string(file_path) {
            Ok(content) => content,
            Err(e) => {
                warn!("Failed to read file {:?}: {}", file_path, e);
                continue;
            }
        };
        
        // Compute checksum
        let checksum = compute_checksum(&content);
        new_checksums.insert(relative_path.clone(), checksum.clone());
        
        // Check if file has changed
        if let Some(old_checksum) = state.file_checksums.get(&relative_path) {
            if old_checksum == &checksum {
                debug!("File unchanged, skipping: {:?}", file_path);
                continue;
            }
        }
        
        info!("Processing new/changed file: {:?}", file_path);
        
        // Process the file content
        let processed_content = process_file_content(&content, file_path)?;
        
        // Create chunks
        let chunks = create_chunks(&processed_content, &relative_path, cli.chunk_size, cli.chunk_overlap)?;
        
        debug!("Created {} chunks for file: {:?}", chunks.len(), file_path);
        all_chunks.extend(chunks);
    }
    
    // Update state with new checksums
    state.file_checksums = new_checksums;
    state.save()
        .context("Failed to save ingestion state")?;
    
    info!("✅ Ingested {} chunks from {} files", all_chunks.len(), state.file_checksums.len());
    
    Ok(all_chunks)
}

/// Check if a file is supported for ingestion
fn is_supported_file(path: &Path) -> bool {
    if let Some(extension) = path.extension() {
        let ext = extension.to_string_lossy().to_lowercase();
        matches!(ext.as_str(), "md" | "markdown" | "html" | "htm" | "txt" | "rst")
    } else {
        false
    }
}

/// Compute SHA-256 checksum of content
fn compute_checksum(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Process file content based on file type
fn process_file_content(content: &str, file_path: &Path) -> Result<String> {
    let extension = file_path.extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_lowercase();
    
    match extension.as_str() {
        "md" | "markdown" => process_markdown(content),
        "html" | "htm" => process_html(content),
        "txt" | "rst" => Ok(process_plain_text(content)),
        _ => Ok(process_plain_text(content)),
    }
}

/// Process Markdown content
fn process_markdown(content: &str) -> Result<String> {
    // Strip frontmatter if present
    let content = strip_frontmatter(content);
    
    // Simple markdown processing - remove common markdown syntax
    let processed = content
        .lines()
        .map(|line| {
            let line = line.trim();
            
            // Remove heading markers
            if line.starts_with('#') {
                return line.trim_start_matches('#').trim().to_string();
            }
            
            // Remove code block markers
            if line.starts_with("```") {
                return String::new();
            }
            
            // Remove bold/italic markers (basic)
            let line = line.replace("**", "").replace("*", "");
            
            // Remove inline code markers
            let line = line.replace("`", "");
            
            line
        })
        .collect::<Vec<_>>()
        .join(" ");
    
    // Normalize whitespace
    let normalized = normalize_whitespace(&processed);
    
    Ok(normalized)
}

/// Process HTML content
fn process_html(content: &str) -> Result<String> {
    // Basic HTML tag removal - in a real implementation, you'd use an HTML parser
    // like scraper or html5ever for proper parsing
    let mut processed = content.to_string();
    
    // Remove common HTML tags (very basic approach)
    let tags_to_remove = [
        "<script", "</script>", "<style", "</style>", "<nav", "</nav>",
        "<header", "</header>", "<footer", "</footer>", "<aside", "</aside>",
    ];
    
    for tag in &tags_to_remove {
        // This is a very basic approach - a real implementation would use proper HTML parsing
        while let Some(start) = processed.find(tag) {
            if let Some(end) = processed[start..].find('>') {
                let tag_end = start + end + 1;
                processed.drain(start..tag_end);
                processed.insert(start, ' ');
            } else {
                break;
            }
        }
    }
    
    // Remove remaining HTML tags
    processed = regex::Regex::new(r"<[^>]*>")
        .map_err(|e| anyhow!("Regex error: {}", e))?
        .replace_all(&processed, " ")
        .to_string();
    
    // Decode HTML entities (basic)
    processed = processed
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");
    
    Ok(normalize_whitespace(&processed))
}

/// Process plain text content
fn process_plain_text(content: &str) -> String {
    normalize_whitespace(content)
}

/// Strip YAML frontmatter from markdown content
fn strip_frontmatter(content: &str) -> &str {
    if content.starts_with("---") {
        if let Some(end) = content[3..].find("---") {
            let frontmatter_end = end + 6; // 3 for first "---" + 3 for second "---"
            if frontmatter_end < content.len() {
                return &content[frontmatter_end..];
            }
        }
    }
    content
}

/// Normalize whitespace in text
fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

/// Create chunks from processed content
fn create_chunks(
    content: &str,
    source: &str,
    chunk_size: usize,
    chunk_overlap: usize,
) -> Result<Vec<Chunk>> {
    if content.is_empty() {
        return Ok(Vec::new());
    }
    
    if chunk_size <= chunk_overlap {
        return Err(anyhow!("chunk_size must be greater than chunk_overlap"));
    }
    
    let tokens: Vec<&str> = content.split_whitespace().collect();
    
    if tokens.is_empty() {
        return Ok(Vec::new());
    }
    
    let mut chunks = Vec::new();
    let mut start = 0;
    let mut chunk_index = 0;
    
    while start < tokens.len() {
        let end = std::cmp::min(start + chunk_size, tokens.len());
        let chunk_tokens = &tokens[start..end];
        let chunk_text = chunk_tokens.join(" ");
        
        // Skip very short chunks (less than 10 tokens)
        if chunk_tokens.len() >= 10 {
            let chunk = Chunk {
                id: format!("{}:chunk{}", source, chunk_index),
                text: chunk_text,
                source: source.to_string(),
                heading: None, // TODO: Extract headings from content
                position: chunk_index,
            };
            
            chunks.push(chunk);
            chunk_index += 1;
        }
        
        // Move start position forward
        if end >= tokens.len() {
            break;
        }
        
        start = if chunk_size > chunk_overlap {
            start + chunk_size - chunk_overlap
        } else {
            start + 1
        };
    }
    
    Ok(chunks)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_normalize_whitespace() {
        let input = "  hello   world  \n  foo   bar  ";
        let expected = "hello world foo bar";
        assert_eq!(normalize_whitespace(input), expected);
    }
    
    #[test]
    fn test_strip_frontmatter() {
        let input = "---\ntitle: Test\n---\n\nContent here";
        let expected = "\n\nContent here";
        assert_eq!(strip_frontmatter(input), expected);
    }
    
    #[test]
    fn test_create_chunks() {
        let content = "This is a test content with many words that should be split into chunks";
        let chunks = create_chunks(content, "test.md", 5, 1).unwrap();
        
        assert!(!chunks.is_empty());
        assert_eq!(chunks[0].source, "test.md");
        assert_eq!(chunks[0].position, 0);
        assert!(chunks[0].id.starts_with("test.md:chunk"));
    }
    
    #[test]
    fn test_is_supported_file() {
        assert!(is_supported_file(Path::new("test.md")));
        assert!(is_supported_file(Path::new("test.html")));
        assert!(is_supported_file(Path::new("test.txt")));
        assert!(!is_supported_file(Path::new("test.pdf")));
        assert!(!is_supported_file(Path::new("test.jpg")));
    }
    
    #[test]
    fn test_compute_checksum() {
        let content = "test content";
        let checksum1 = compute_checksum(content);
        let checksum2 = compute_checksum(content);
        let checksum3 = compute_checksum("different content");
        
        assert_eq!(checksum1, checksum2);
        assert_ne!(checksum1, checksum3);
    }
    
    #[test]
    fn test_process_markdown() {
        let markdown = "# Header\n\nThis is **bold** text with `code`.\n\n```rust\nfn main() {}\n```\n\nMore text.";
        let processed = process_markdown(markdown).unwrap();
        
        assert!(!processed.contains('#'));
        assert!(!processed.contains("**"));
        assert!(!processed.contains('`'));
        assert!(processed.contains("Header"));
        assert!(processed.contains("bold"));
        assert!(processed.contains("code"));
    }
}