// src/retriever.rs

//! Retrieval module for Basic RAG
//!
//! This module implements BM25-based search over the Tantivy index.
//! It retrieves the most relevant document chunks for a given query.

use anyhow::{Context, Result};
use log::{debug, warn, info};
use tantivy::schema::{Field, Value};
use tantivy::{
    collector::TopDocs,
    query::QueryParser,
    Index as TantivyIndex,
    IndexReader,
    ReloadPolicy,
    Searcher,
    TantivyDocument,
};
use crate::ingest::Chunk;

/// Wrapper around Tantivy Index with cached field handles
pub struct Index {
    pub tantivy_index: TantivyIndex,
    pub reader: IndexReader,
    pub id_field: Field,
    pub text_field: Field,
    pub source_field: Field,
    pub position_field: Field,
    pub heading_field: Field,
}

impl Index {
    /// Create a new Index wrapper from a Tantivy index
    pub fn new(tantivy_index: TantivyIndex) -> Result<Self> {
        info!("Index::new - Creating new Index wrapper");
        debug!("Index::new - Building index reader with OnCommitWithDelay policy");
        
        let reader = tantivy_index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .context("Failed to create index reader")?;
        debug!("Index::new - Index reader created successfully");

        debug!("Index::new - Extracting schema from tantivy index");
        let schema = tantivy_index.schema();
        
        // Get field handles - these should match the schema in indexer.rs
        debug!("Index::new - Looking up field handles in schema");
        let id_field = schema
            .get_field("id")
            .context("Index schema missing 'id' field")?;
        debug!("Index::new - Found 'id' field");
        
        let text_field = schema
            .get_field("text")
            .context("Index schema missing 'text' field")?;
        debug!("Index::new - Found 'text' field");
        
        let source_field = schema
            .get_field("source")
            .context("Index schema missing 'source' field")?;
        debug!("Index::new - Found 'source' field");
        
        let position_field = schema
            .get_field("position")
            .context("Index schema missing 'position' field")?;
        debug!("Index::new - Found 'position' field");
        
        let heading_field = schema
            .get_field("heading")
            .context("Index schema missing 'heading' field")?;
        debug!("Index::new - Found 'heading' field");

        info!("Index::new - Successfully created Index wrapper with all required fields");
        Ok(Index {
            tantivy_index,
            reader,
            id_field,
            text_field,
            source_field,
            position_field,
            heading_field,
        })
    }

    /// Get a searcher for the current index state
    pub fn searcher(&self) -> Searcher {
        debug!("Index::searcher - Creating new searcher instance");
        let searcher = self.reader.searcher();
        debug!("Index::searcher - Searcher created successfully");
        searcher
    }
}

/// Perform BM25 search on the index and return the top-K matching chunks.
///
/// This function:
/// 1. Parses the query string using Tantivy's QueryParser
/// 2. Executes BM25 search with the specified limit
/// 3. Retrieves matching documents and converts them back to Chunk objects
/// 4. Returns chunks sorted by relevance score (highest first)
pub fn bm25_search(
    index: &Index,
    query: &str,
    top_k: usize,
) -> Result<Vec<Chunk>> {
    info!("bm25_search - Starting BM25 search for query: '{}' (top {})", query, top_k);
    debug!("bm25_search - Query length: {} characters", query.len());

    if query.trim().is_empty() {
        warn!("bm25_search - Empty query provided, returning empty results");
        return Ok(Vec::new());
    }

    // Get a searcher for the current index state
    debug!("bm25_search - Getting searcher for current index state");
    let searcher = index.searcher();
    debug!("bm25_search - Searcher obtained successfully");
    
    // Create a query parser that searches over the text field
    debug!("bm25_search - Creating query parser for text field");
    let query_parser = QueryParser::for_index(&index.tantivy_index, vec![index.text_field]);
    debug!("bm25_search - Query parser created successfully");
    
    // Parse the query string - handle parse errors gracefully
    debug!("bm25_search - Attempting to parse query: '{}'", query);
    let parsed_query = match query_parser.parse_query(query) {
        Ok(q) => {
            debug!("bm25_search - Query parsed successfully");
            q
        },
        Err(e) => {
            // If parsing fails, try to create a simple term query
            warn!("bm25_search - Failed to parse query '{}': {}. Trying fallback approach.", query, e);
            
            // Fallback: treat the entire query as a phrase or term
            debug!("bm25_search - Sanitizing query for fallback parsing");
            let sanitized_query = sanitize_query(query);
            debug!("bm25_search - Sanitized query: '{}'", sanitized_query);
            
            query_parser.parse_query(&sanitized_query)
                .with_context(|| format!("Failed to parse sanitized query '{}'", sanitized_query))?
        }
    };

    // Execute the search with BM25 scoring
    info!("bm25_search - Executing BM25 search with limit: {}", top_k);
    let top_docs = searcher
        .search(&parsed_query, &TopDocs::with_limit(top_k))
        .context("Failed to execute search")?;

    info!("bm25_search - Search completed, found {} results", top_docs.len());
    debug!("bm25_search - Converting search results to chunks");

    // Convert search results back to Chunk objects
    let mut chunks = Vec::with_capacity(top_docs.len());
    debug!("bm25_search - Allocated vector with capacity: {}", top_docs.len());
    
    for (i, (score, doc_address)) in top_docs.iter().enumerate() {
        debug!("bm25_search - Processing result {} with score: {}", i + 1, score);
        
        // Retrieve the document from the index
        debug!("bm25_search - Retrieving document at address: {:?}", doc_address);
        let retrieved_doc = searcher
            .doc(*doc_address)
            .context("Failed to retrieve document")?;
        debug!("bm25_search - Document retrieved successfully");

        // Extract the chunk from the document
        debug!("bm25_search - Converting document to chunk");
        match document_to_chunk(&retrieved_doc, index, *score) {
            Ok(chunk) => {
                debug!("bm25_search - Successfully converted document to chunk with ID: {}", chunk.id);
                chunks.push(chunk);
            },
            Err(e) => {
                warn!("bm25_search - Failed to convert document to chunk: {}", e);
                continue;
            }
        }
    }

    info!("bm25_search - Successfully converted {} documents to chunks", chunks.len());
    debug!("bm25_search - Search operation completed successfully");
    Ok(chunks)
}

/// Convert a Tantivy Document back to a Chunk object
fn document_to_chunk(doc: &TantivyDocument, index: &Index, score: f32) -> Result<Chunk> {
    debug!("document_to_chunk - Converting document with score: {}", score);
    
    // Helper function to extract string from document field
    let extract_string = |field: Field, field_name: &str| -> Result<String> {
        doc.get_first(field)
            .ok_or_else(|| anyhow::anyhow!("Document missing '{}' field", field_name))
            .and_then(|v| {
                // Try to convert to string - handle different value types
                if let Some(text) = v.as_value().as_str() {
                    Ok(text.to_string())
                } else {
                    Err(anyhow::anyhow!("Field '{}' is not a string", field_name))
                }
            })
    };
    
    // Helper function to extract u64 from document field
    let extract_u64 = |field: Field, field_name: &str| -> Result<u64> {
        doc.get_first(field)
            .ok_or_else(|| anyhow::anyhow!("Document missing '{}' field", field_name))
            .and_then(|v| {
                if let Some(num) = v.as_value().as_u64() {
                    Ok(num)
                } else {
                    Err(anyhow::anyhow!("Field '{}' is not a u64", field_name))
                }
            })
    };
    
    // Extract stored fields from the document
    debug!("document_to_chunk - Extracting 'id' field");
    let id = extract_string(index.id_field, "id")?;
    debug!("document_to_chunk - Found ID: {}", id);

    debug!("document_to_chunk - Extracting 'text' field");
    let text = extract_string(index.text_field, "text")?;
    debug!("document_to_chunk - Found text with length: {} characters", text.len());

    debug!("document_to_chunk - Extracting 'source' field");
    let source = extract_string(index.source_field, "source")?;
    debug!("document_to_chunk - Found source: {}", source);

    debug!("document_to_chunk - Extracting 'position' field");
    let position = extract_u64(index.position_field, "position")? as usize;
    debug!("document_to_chunk - Found position: {}", position);

    // Heading is optional - it might be None for some chunks
    debug!("document_to_chunk - Extracting optional 'heading' field");
    let heading = doc
        .get_first(index.heading_field)
        .and_then(|v| {
            v.as_value().as_str()
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
        });
    
    match &heading {
        Some(h) => debug!("document_to_chunk - Found heading: {}", h),
        None => debug!("document_to_chunk - No heading found (optional field)"),
    }

    debug!("document_to_chunk - Successfully extracted all fields, creating chunk");
    Ok(Chunk {
        id,
        text,
        source,
        heading,
        position,
    })
}

/// Sanitize a query string to make it more likely to parse successfully
fn sanitize_query(query: &str) -> String {
    debug!("sanitize_query - Sanitizing query: '{}'", query);
    
    // Remove or escape special characters that might cause parsing issues
    let mut sanitized = query.to_string();
    debug!("sanitize_query - Starting with: '{}'", sanitized);
    
    // Remove characters that commonly cause issues in query parsing
    let problematic_chars = ['[', ']', '{', '}', '(', ')', '~', '^'];
    debug!("sanitize_query - Removing problematic characters: {:?}", problematic_chars);
    
    for ch in problematic_chars {
        let before_count = sanitized.matches(ch).count();
        sanitized = sanitized.replace(ch, "");
        if before_count > 0 {
            debug!("sanitize_query - Removed {} instances of '{}'", before_count, ch);
        }
    }
    
    // Escape quotes by removing them (simpler approach)
    debug!("sanitize_query - Removing quote characters");
    let quote_count = sanitized.matches('"').count() + sanitized.matches('\'').count();
    sanitized = sanitized.replace('"', "");
    sanitized = sanitized.replace('\'', "");
    if quote_count > 0 {
        debug!("sanitize_query - Removed {} quote characters", quote_count);
    }
    
    // Clean up multiple spaces
    debug!("sanitize_query - Cleaning up whitespace");
    let before_spaces = sanitized.len();
    sanitized = sanitized
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    debug!("sanitize_query - Whitespace cleanup: {} -> {} characters", before_spaces, sanitized.len());
    
    // If the sanitized query is empty, return a wildcard
    let result = if sanitized.trim().is_empty() {
        debug!("sanitize_query - Query became empty after sanitization, using wildcard");
        "*".to_string()
    } else {
        debug!("sanitize_query - Sanitization complete: '{}'", sanitized);
        sanitized
    };
    
    debug!("sanitize_query - Final sanitized query: '{}'", result);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_query() {
        debug!("test_sanitize_query - Running sanitization tests");
        
        assert_eq!(sanitize_query("hello world"), "hello world");
        debug!("test_sanitize_query - Basic query test passed");
        
        assert_eq!(sanitize_query("hello [world]"), "hello world");
        debug!("test_sanitize_query - Square brackets test passed");
        
        assert_eq!(sanitize_query("hello {world}"), "hello world");
        debug!("test_sanitize_query - Curly braces test passed");
        
        assert_eq!(sanitize_query("hello (world)"), "hello world");
        debug!("test_sanitize_query - Parentheses test passed");
        
        assert_eq!(sanitize_query("hello \"world\""), "hello world");
        debug!("test_sanitize_query - Double quotes test passed");
        
        assert_eq!(sanitize_query("hello 'world'"), "hello world");
        debug!("test_sanitize_query - Single quotes test passed");
        
        assert_eq!(sanitize_query("   hello    world   "), "hello world");
        debug!("test_sanitize_query - Whitespace cleanup test passed");
        
        assert_eq!(sanitize_query(""), "*");
        debug!("test_sanitize_query - Empty string test passed");
        
        assert_eq!(sanitize_query("   "), "*");
        debug!("test_sanitize_query - Whitespace-only string test passed");
        
        debug!("test_sanitize_query - All sanitization tests passed");
    }

    #[test]
    fn test_empty_query() {
        debug!("test_empty_query - Testing empty query handling");
        // This test would require setting up a real index, so we'll just test
        // that the function handles empty strings correctly
        assert_eq!(sanitize_query(""), "*");
        debug!("test_empty_query - Empty query test passed");
    }
}