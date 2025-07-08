// src/indexer.rs

//! Indexer module for Basic RAG
//!
//! This module handles creating, updating, and opening Tantivy indexes for document chunks.
//! It supports both full index rebuilds and incremental updates based on chunk state tracking.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tantivy::{
    doc,
    schema::{Field, Schema, TextFieldIndexing, TextOptions, FAST, INDEXED, STORED, STRING},
    Index as TantivyIndex, IndexReader, IndexWriter, ReloadPolicy, Searcher, Term,
};

use crate::cli::Cli;
use crate::ingest::Chunk;

/// Index state tracking for incremental updates
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IndexState {
    /// Maps chunk ID to its content hash for change detection
    pub chunk_hashes: HashMap<String, String>,
    /// Schema version for compatibility checking
    pub schema_version: u32,
    /// Last update timestamp
    pub last_updated: u64,
}

/// Wrapper around Tantivy index with schema field handles
pub struct Index {
    pub tantivy_index: TantivyIndex,
    pub _schema: Schema,
    pub id_field: Field,
    pub text_field: Field,
    pub source_field: Field,
    pub heading_field: Field,
    pub position_field: Field,
    pub _reader: IndexReader,
}

impl Index {
    /// Create a new index with the defined schema
    pub fn create_in_dir<P: AsRef<Path>>(dir: P) -> Result<Self> {
        let schema = build_schema();
        let tantivy_index = TantivyIndex::create_in_dir(dir, schema.clone())
            .context("Failed to create Tantivy index")?;
        
        let id_field = schema.get_field("id").unwrap();
        let text_field = schema.get_field("text").unwrap();
        let source_field = schema.get_field("source").unwrap();
        let heading_field = schema.get_field("heading").unwrap();
        let position_field = schema.get_field("position").unwrap();
        
        let reader = tantivy_index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .context("Failed to create index reader")?;

        Ok(Index {
            tantivy_index,
            _schema: schema,
            id_field,
            text_field,
            source_field,
            heading_field,
            position_field,
            _reader: reader,
        })
    }

    /// Open an existing index
    pub fn open_in_dir<P: AsRef<Path>>(dir: P) -> Result<Self> {
        let tantivy_index = TantivyIndex::open_in_dir(dir)
            .context("Failed to open Tantivy index")?;
        
        let schema = tantivy_index.schema();
        let id_field = schema.get_field("id").unwrap();
        let text_field = schema.get_field("text").unwrap();
        let source_field = schema.get_field("source").unwrap();
        let heading_field = schema.get_field("heading").unwrap();
        let position_field = schema.get_field("position").unwrap();
        
        let reader = tantivy_index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .context("Failed to create index reader")?;

        Ok(Index {
            tantivy_index,
            _schema: schema,
            id_field,
            text_field,
            source_field,
            heading_field,
            position_field,
            _reader: reader,
        })
    }

    /// Get a searcher for querying the index
    pub fn _searcher(&self) -> Searcher {
        self._reader.searcher()
    }

    /// Get an index writer with appropriate heap size
    pub fn writer(&self, heap_size: usize) -> Result<IndexWriter> {
        self.tantivy_index
            .writer(heap_size)
            .context("Failed to create index writer")
    }
}

/// Build the Tantivy schema for document chunks
fn build_schema() -> Schema {
    let mut schema_builder = Schema::builder();
    
    // ID field: unique identifier for each chunk
    schema_builder.add_text_field("id", STRING | STORED);
    
    // Text field: the main content for full-text search with BM25
    let text_indexing = TextFieldIndexing::default()
        .set_tokenizer("en_stem")
        .set_index_option(tantivy::schema::IndexRecordOption::WithFreqsAndPositions);
    let text_options = TextOptions::default()
        .set_indexing_options(text_indexing)
        .set_stored();
    schema_builder.add_text_field("text", text_options);
    
    // Source field: file path or URL for traceability
    schema_builder.add_text_field("source", STRING | STORED);
    
    // Heading field: optional heading context
    schema_builder.add_text_field("heading", STRING | STORED);
    
    // Position field: chunk position within the source file
    schema_builder.add_u64_field("position", INDEXED | STORED | FAST);
    
    schema_builder.build()
}

/// Build or update the Tantivy index from chunks
pub fn build_index(cli: &Cli, chunks: &[Chunk]) -> Result<()> {
    log::info!("Building index at {:?} with {} chunks", cli.index_dir, chunks.len());
    
    // Create index directory if it doesn't exist
    fs::create_dir_all(&cli.index_dir)
        .context("Failed to create index directory")?;
    
    // Load existing state or create new one
    let state_path = cli.index_dir.join("state.json");
    let mut state = load_index_state(&state_path).unwrap_or_default();
    
    // Create hash map of current chunks for efficient lookup
    let current_chunks: HashMap<String, &Chunk> = chunks.iter()
        .map(|chunk| (chunk.id.clone(), chunk))
        .collect();
    
    // Determine which chunks are new, modified, or removed
    let mut new_chunks = Vec::new();
    let mut modified_chunks = Vec::new();
    let mut removed_chunk_ids = Vec::new();
    
    // Check for new and modified chunks
    for chunk in chunks {
        let chunk_hash = calculate_chunk_hash(chunk);
        match state.chunk_hashes.get(&chunk.id) {
            Some(existing_hash) if existing_hash == &chunk_hash => {
                // Chunk unchanged, skip
                continue;
            }
            Some(_) => {
                // Chunk modified
                modified_chunks.push(chunk);
                state.chunk_hashes.insert(chunk.id.clone(), chunk_hash);
            }
            None => {
                // New chunk
                new_chunks.push(chunk);
                state.chunk_hashes.insert(chunk.id.clone(), chunk_hash);
            }
        }
    }
    
    // Check for removed chunks
    for existing_id in state.chunk_hashes.keys() {
        if !current_chunks.contains_key(existing_id) {
            removed_chunk_ids.push(existing_id.clone());
        }
    }
    
    // Remove deleted chunks from state
    for id in &removed_chunk_ids {
        state.chunk_hashes.remove(id);
    }
    
    log::info!(
        "Index update: {} new, {} modified, {} removed chunks",
        new_chunks.len(),
        modified_chunks.len(),
        removed_chunk_ids.len()
    );
    
    // If no changes, skip index update
    if new_chunks.is_empty() && modified_chunks.is_empty() && removed_chunk_ids.is_empty() {
        log::info!("No changes detected, skipping index update");
        return Ok(());
    }
    
    // Create or open the index
    let index = if cli.index_dir.join("meta.json").exists() {
        Index::open_in_dir(&cli.index_dir)?
    } else {
        Index::create_in_dir(&cli.index_dir)?
    };
    
    // Get index writer with 50MB heap
    let mut writer = index.writer(50_000_000)?;
    
    // Remove deleted chunks
    for chunk_id in &removed_chunk_ids {
        let term = Term::from_field_text(index.id_field, chunk_id);
        writer.delete_term(term);
        log::debug!("Deleted chunk: {}", chunk_id);
    }
    
    // Add new chunks
    for chunk in &new_chunks {
        add_chunk_to_writer(&mut writer, &index, chunk)?;
        log::debug!("Added new chunk: {}", chunk.id);
    }
    
    // Update modified chunks (delete old + add new)
    for chunk in &modified_chunks {
        let term = Term::from_field_text(index.id_field, &chunk.id);
        writer.delete_term(term);
        add_chunk_to_writer(&mut writer, &index, chunk)?;
        log::debug!("Updated chunk: {}", chunk.id);
    }
    
    // Commit changes
    writer.commit().context("Failed to commit index changes")?;
    
    // Update state metadata
    state.schema_version = 1;
    state.last_updated = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    // Save updated state
    save_index_state(&state_path, &state)?;
    
    log::info!("Index successfully built/updated");
    Ok(())
}

/// Open an existing Tantivy index for querying
pub fn open_index(cli: &Cli) -> Result<Index> {
    log::info!("Opening index at {:?}", cli.index_dir);
    
    if !cli.index_dir.exists() {
        anyhow::bail!("Index directory does not exist: {:?}", cli.index_dir);
    }
    
    Index::open_in_dir(&cli.index_dir)
        .context("Failed to open index")
}

/// Add a chunk to the index writer
fn add_chunk_to_writer(writer: &mut IndexWriter, index: &Index, chunk: &Chunk) -> Result<()> {
    let mut doc = tantivy::TantivyDocument::default();
    
    doc.add_text(index.id_field, &chunk.id);
    doc.add_text(index.text_field, &chunk.text);
    doc.add_text(index.source_field, &chunk.source);
    
    if let Some(heading) = &chunk.heading {
        doc.add_text(index.heading_field, heading);
    } else {
        doc.add_text(index.heading_field, "");
    }
    
    doc.add_u64(index.position_field, chunk.position as u64);
    
    writer.add_document(doc)?;
    Ok(())
}

/// Calculate a simple hash for a chunk to detect changes
fn calculate_chunk_hash(chunk: &Chunk) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    
    let mut hasher = DefaultHasher::new();
    chunk.text.hash(&mut hasher);
    chunk.source.hash(&mut hasher);
    chunk.heading.hash(&mut hasher);
    chunk.position.hash(&mut hasher);
    
    format!("{:x}", hasher.finish())
}

/// Load index state from JSON file
fn load_index_state(path: &Path) -> Result<IndexState> {
    if !path.exists() {
        return Ok(IndexState::default());
    }
    
    let content = fs::read_to_string(path)
        .context("Failed to read index state file")?;
    
    serde_json::from_str(&content)
        .context("Failed to parse index state JSON")
}

/// Save index state to JSON file
fn save_index_state(path: &Path, state: &IndexState) -> Result<()> {
    let content = serde_json::to_string_pretty(state)
        .context("Failed to serialize index state")?;
    
    fs::write(path, content)
        .context("Failed to write index state file")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    fn create_test_chunk(id: &str, text: &str, source: &str, position: usize) -> Chunk {
        Chunk {
            id: id.to_string(),
            text: text.to_string(),
            source: source.to_string(),
            heading: None,
            position,
        }
    }
    
    #[test]
    fn test_schema_creation() {
        let schema = build_schema();
        assert!(schema.get_field("id").is_ok());
        assert!(schema.get_field("text").is_ok());
        assert!(schema.get_field("source").is_ok());
        assert!(schema.get_field("heading").is_ok());
        assert!(schema.get_field("position").is_ok());
    }
    
    #[test]
    fn test_index_creation() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let index = Index::create_in_dir(temp_dir.path())?;
        
        // Test that we can create a writer
        let _writer = index.writer(1_000_000)?;
        
        Ok(())
    }
    
    #[test]
    fn test_chunk_hash_consistency() {
        let chunk1 = create_test_chunk("test1", "content", "source.md", 0);
        let chunk2 = create_test_chunk("test1", "content", "source.md", 0);
        let chunk3 = create_test_chunk("test1", "different", "source.md", 0);
        
        assert_eq!(calculate_chunk_hash(&chunk1), calculate_chunk_hash(&chunk2));
        assert_ne!(calculate_chunk_hash(&chunk1), calculate_chunk_hash(&chunk3));
    }
    
    #[test]
    fn test_index_state_serialization() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let state_path = temp_dir.path().join("state.json");
        
        let mut state = IndexState::default();
        state.chunk_hashes.insert("test1".to_string(), "hash1".to_string());
        state.schema_version = 1;
        state.last_updated = 123456;
        
        save_index_state(&state_path, &state)?;
        let loaded_state = load_index_state(&state_path)?;
        
        assert_eq!(state.chunk_hashes, loaded_state.chunk_hashes);
        assert_eq!(state.schema_version, loaded_state.schema_version);
        assert_eq!(state.last_updated, loaded_state.last_updated);
        
        Ok(())
    }
}