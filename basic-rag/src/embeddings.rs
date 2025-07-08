// src/embeddings.rs - New module for embedding functionality

//! Embedding module for semantic search capabilities
//!
//! This module handles text embeddings for semantic similarity search,
//! integrating with the existing BM25 search for hybrid retrieval.

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Command;
use std::fs;
use std::path::Path;
use log::{info, debug, warn};
use crate::ingest::Chunk;

/// Enhanced chunk with embedding capability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhancedChunk {
    pub id: String,
    pub text: String,
    pub source: String,
    pub heading: Option<String>,
    pub position: usize,
    pub embedding: Option<Vec<f32>>,
}

impl From<Chunk> for EnhancedChunk {
    fn from(chunk: Chunk) -> Self {
        Self {
            id: chunk.id,
            text: chunk.text,
            source: chunk.source,
            heading: chunk.heading,
            position: chunk.position,
            embedding: None,
        }
    }
}

impl From<&EnhancedChunk> for Chunk {
    fn from(enhanced: &EnhancedChunk) -> Self {
        Self {
            id: enhanced.id.clone(),
            text: enhanced.text.clone(),
            source: enhanced.source.clone(),
            heading: enhanced.heading.clone(),
            position: enhanced.position,
        }
    }
}

/// Search result with hybrid scoring
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SearchResult {
    pub chunk: EnhancedChunk,
    pub bm25_score: f32,
    pub semantic_score: f32,
    pub combined_score: f32,
    pub explanation: String,
}

/// Query analysis for search strategy selection
#[derive(Debug)]
#[allow(dead_code)]
pub enum SearchStrategy {
    BM25Heavy { alpha: f32 },    // alpha = 0.8 (80% BM25, 20% semantic)
    Balanced { alpha: f32 },     // alpha = 0.6 (60% BM25, 40% semantic)
    SemanticHeavy { alpha: f32 }, // alpha = 0.3 (30% BM25, 70% semantic)
    PureBM25,
    PureSemantic,
}

/// Simple embedding model using sentence-transformers via Python
pub struct EmbeddingModel {
    python_script: String,
}

impl EmbeddingModel {
    pub fn new() -> Result<Self> {
        let python_script = Self::create_embedding_script()?;
        
        // Test if the model works
        Self::test_python_embedding(&python_script)?;
        
        Ok(Self { python_script })
    }
    
    fn create_embedding_script() -> Result<String> {
        let script = r#"
import sys
import json
import numpy as np
from sentence_transformers import SentenceTransformer

# Load a lightweight model (384 dimensions)
model = SentenceTransformer('all-MiniLM-L6-v2')

def encode_text(text):
    """Encode text to embedding vector"""
    embedding = model.encode([text])
    return embedding[0].tolist()

if __name__ == "__main__":
    if len(sys.argv) != 2:
        print("Usage: python embed.py <text>", file=sys.stderr)
        sys.exit(1)
    
    text = sys.argv[1]
    try:
        embedding = encode_text(text)
        print(json.dumps(embedding))
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)
"#;
        
        let script_path = "embed.py";
        fs::write(script_path, script)
            .context("Failed to write embedding script")?;
        
        Ok(script_path.to_string())
    }
    
    fn test_python_embedding(script_path: &str) -> Result<()> {
        info!("Testing Python embedding model...");
        
        let output = Command::new("python3")
            .arg(script_path)
            .arg("test text")
            .output()
            .context("Failed to execute Python embedding script. Make sure Python 3 and sentence-transformers are installed.")?;
        
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!(
                "Python embedding script failed: {}. Try: pip install sentence-transformers", 
                stderr
            ));
        }
        
        info!("✅ Python embedding model working correctly");
        Ok(())
    }
    
    pub fn encode(&self, text: &str) -> Result<Vec<f32>> {
        debug!("Encoding text: {} chars", text.len());
        
        let output = Command::new("python3")
            .arg(&self.python_script)
            .arg(text)
            .output()
            .context("Failed to execute embedding script")?;
        
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("Embedding failed: {}", stderr));
        }
        
        let stdout = String::from_utf8_lossy(&output.stdout);
        let embedding: Vec<f32> = serde_json::from_str(&stdout)
            .context("Failed to parse embedding JSON")?;
        
        debug!("Generated embedding with {} dimensions", embedding.len());
        Ok(embedding)
    }
    
    pub fn _dimension(&self) -> usize {
        384 // all-MiniLM-L6-v2 dimension
    }
}

/// In-memory embedding store
pub struct EmbeddingStore {
    embeddings: HashMap<String, Vec<f32>>,
    chunks: HashMap<String, EnhancedChunk>,
}

impl EmbeddingStore {
    pub fn new() -> Self {
        Self {
            embeddings: HashMap::new(),
            chunks: HashMap::new(),
        }
    }
    
    pub fn add_chunk(&mut self, chunk: EnhancedChunk) {
        if let Some(embedding) = &chunk.embedding {
            self.embeddings.insert(chunk.id.clone(), embedding.clone());
        }
        self.chunks.insert(chunk.id.clone(), chunk);
    }
    
    pub fn similarity_search(&self, query_embedding: &[f32], top_k: usize) -> Vec<(EnhancedChunk, f32)> {
        let mut scored_chunks = Vec::new();
        
        for (chunk_id, chunk_embedding) in &self.embeddings {
            let score = cosine_similarity(query_embedding, chunk_embedding);
            if let Some(chunk) = self.chunks.get(chunk_id) {
                scored_chunks.push((chunk.clone(), score));
            }
        }
        
        scored_chunks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        scored_chunks.truncate(top_k);
        
        scored_chunks
    }
    
    pub fn save_to_disk(&self, path: &Path) -> Result<()> {
        let data = serde_json::to_string_pretty(&self.chunks)
            .context("Failed to serialize chunks")?;
        
        fs::write(path, data)
            .context("Failed to write embeddings to disk")?;
        
        info!("Saved {} chunks with embeddings to disk", self.chunks.len());
        Ok(())
    }
    
    pub fn load_from_disk(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::new());
        }
        
        let data = fs::read_to_string(path)
            .context("Failed to read embeddings file")?;
        
        let chunks: HashMap<String, EnhancedChunk> = serde_json::from_str(&data)
            .context("Failed to parse embeddings JSON")?;
        
        let mut store = Self::new();
        let mut embedding_count = 0;
        
        for chunk in chunks.into_values() {
            if chunk.embedding.is_some() {
                embedding_count += 1;
            }
            store.add_chunk(chunk);
        }
        
        info!("Loaded {} chunks ({} with embeddings) from disk", store.chunks.len(), embedding_count);
        Ok(store)
    }
}

/// Utility functions
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    
    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot_product / (norm_a * norm_b)
    }
}

pub fn normalize_bm25_score(score: f32) -> f32 {
    // Sigmoid normalization to [0,1]
    1.0 / (1.0 + (-score / 5.0).exp())
}

pub fn analyze_query(query: &str) -> SearchStrategy {
    let query_lower = query.to_lowercase();
    let word_count = query.split_whitespace().count();
    
    // Check for technical/API patterns
    if query_lower.contains("::") || 
       query_lower.contains("api") ||
       query_lower.contains("function") ||
       query_lower.contains("struct") ||
       query_lower.contains("impl") ||
       query.chars().any(|c| c == '(' || c == ')' || c == '<' || c == '>') {
        info!("Query appears technical, favoring BM25");
        return SearchStrategy::BM25Heavy { alpha: 0.8 };
    }
    
    // Check for conceptual patterns
    if query_lower.starts_with("how") ||
       query_lower.starts_with("what") ||
       query_lower.starts_with("why") ||
       query_lower.contains("concept") ||
       query_lower.contains("explain") ||
       query_lower.contains("tutorial") {
        info!("Query appears conceptual, favoring semantic search");
        return SearchStrategy::SemanticHeavy { alpha: 0.3 };
    }
    
    // Short queries are often conceptual
    if word_count <= 2 {
        info!("Short query detected, leaning semantic");
        return SearchStrategy::SemanticHeavy { alpha: 0.4 };
    }
    
    // Long queries often mix technical and conceptual
    if word_count > 6 {
        info!("Long query detected, using balanced approach");
        return SearchStrategy::Balanced { alpha: 0.5 };
    }
    
    // Default balanced approach
    info!("Using balanced search strategy");
    SearchStrategy::Balanced { alpha: 0.6 }
}

/// Enhanced indexing with embeddings
pub async fn build_enhanced_index(
    cli: &crate::cli::Cli,
    chunks: &[Chunk],
) -> Result<()> {
    info!("🚀 Building enhanced index with embeddings...");
    
    // First, build the regular BM25 index
    crate::indexer::build_index(cli, chunks)?;
    
    // Initialize embedding model
    let embedding_model = EmbeddingModel::new()
        .context("Failed to initialize embedding model")?;
    
    // Load existing embeddings if available
    let embeddings_path = cli.index_dir.join("embeddings.json");
    let mut embedding_store = EmbeddingStore::load_from_disk(&embeddings_path)
        .unwrap_or_else(|_| EmbeddingStore::new());
    
    // Generate embeddings for new/updated chunks
    let mut enhanced_chunks = Vec::new();
    let mut new_embeddings = 0;
    
    for chunk in chunks {
        let mut enhanced_chunk = EnhancedChunk::from(chunk.clone());
        
        // Check if we already have an embedding for this chunk
        if let Some(existing_chunk) = embedding_store.chunks.get(&chunk.id) {
            if existing_chunk.text == chunk.text {
                // Text hasn't changed, reuse existing embedding
                enhanced_chunk.embedding = existing_chunk.embedding.clone();
                debug!("Reusing embedding for chunk: {}", chunk.id);
            }
        }
        
        // Generate new embedding if needed
        if enhanced_chunk.embedding.is_none() {
            info!("Generating embedding for: {}", chunk.id);
            
            // Combine heading and text for better context
            let text_for_embedding = if let Some(heading) = &chunk.heading {
                format!("{}\n{}", heading, chunk.text)
            } else {
                chunk.text.clone()
            };
            
            match embedding_model.encode(&text_for_embedding) {
                Ok(embedding) => {
                    enhanced_chunk.embedding = Some(embedding);
                    new_embeddings += 1;
                }
                Err(e) => {
                    warn!("Failed to generate embedding for {}: {}", chunk.id, e);
                }
            }
        }
        
        enhanced_chunks.push(enhanced_chunk);
    }
    
    // Update embedding store
    for chunk in enhanced_chunks {
        embedding_store.add_chunk(chunk);
    }
    
    // Save embeddings to disk
    embedding_store.save_to_disk(&embeddings_path)?;
    
    info!("✅ Enhanced index built! Generated {} new embeddings", new_embeddings);
    Ok(())
}

// Updated retriever.rs modifications

/// Hybrid searcher that combines BM25 and semantic search
pub struct HybridSearcher {
    pub bm25_index: crate::retriever::Index,
    pub embedding_store: EmbeddingStore,
    pub embedding_model: EmbeddingModel,
}

impl HybridSearcher {
    pub fn new(cli: &crate::cli::Cli) -> Result<Self> {
        info!("Initializing hybrid searcher...");
        
        // Open BM25 index
        let bm25_index = crate::indexer::open_index(cli)?;
        let bm25_index = crate::retriever::Index::new(bm25_index.tantivy_index)?;
        
        // Load embedding store
        let embeddings_path = cli.index_dir.join("embeddings.json");
        let embedding_store = EmbeddingStore::load_from_disk(&embeddings_path)
            .context("Failed to load embeddings. Run 'build' command first.")?;
        
        // Initialize embedding model
        let embedding_model = EmbeddingModel::new()
            .context("Failed to initialize embedding model")?;
        
        info!("✅ Hybrid searcher initialized");
        Ok(Self {
            bm25_index,
            embedding_store,
            embedding_model,
        })
    }
    
    pub fn hybrid_search(&self, query: &str, top_k: usize) -> Result<Vec<SearchResult>> {
        info!("🔍 Starting hybrid search for: '{}'", query);
        
        // Analyze query to determine strategy
        let strategy = analyze_query(query);
        
        match strategy {
            SearchStrategy::PureBM25 => self.pure_bm25_search(query, top_k),
            SearchStrategy::PureSemantic => self.pure_semantic_search(query, top_k),
            SearchStrategy::BM25Heavy { alpha } |
            SearchStrategy::Balanced { alpha } |
            SearchStrategy::SemanticHeavy { alpha } => {
                self.hybrid_search_with_alpha(query, top_k, alpha)
            }
        }
    }
    
    pub fn hybrid_search_with_alpha(&self, query: &str, top_k: usize, alpha: f32) -> Result<Vec<SearchResult>> {
        info!("Hybrid search with α={:.1} (BM25: {:.0}%, Semantic: {:.0}%)", 
              alpha, alpha * 100.0, (1.0 - alpha) * 100.0);
        
        // Step 1: Get BM25 candidates (cast wider net)
        let bm25_candidates = crate::retriever::bm25_search(&self.bm25_index, query, top_k * 3)?;
        
        // Step 2: Get query embedding
        let query_embedding = self.embedding_model.encode(query)?;
        
        // Step 3: Score all BM25 candidates with semantic similarity
        let mut hybrid_results = Vec::new();
        
        for chunk in bm25_candidates {
            // Get BM25 score (we'll need to modify bm25_search to return scores)
            let bm25_score = 1.0; // Placeholder - we'd need to modify the BM25 function
            
            // Convert to enhanced chunk and get semantic score
            let enhanced_chunk = EnhancedChunk::from(chunk);
            let semantic_score = if let Some(embedding) = &enhanced_chunk.embedding {
                cosine_similarity(&query_embedding, embedding)
            } else {
                0.0
            };
            
            // Combine scores
            let norm_bm25 = normalize_bm25_score(bm25_score);
            let norm_semantic = (semantic_score + 1.0) / 2.0; // Normalize [-1,1] to [0,1]
            let combined_score = alpha * norm_bm25 + (1.0 - alpha) * norm_semantic;
            
            let explanation = format!(
                "BM25: {:.3} (norm: {:.3}), Semantic: {:.3} (norm: {:.3}), α={:.1}",
                bm25_score, norm_bm25, semantic_score, norm_semantic, alpha
            );
            
            hybrid_results.push(SearchResult {
                chunk: enhanced_chunk,
                bm25_score,
                semantic_score,
                combined_score,
                explanation,
            });
        }
        
        // Step 4: Also get some pure semantic results for diversity
        let semantic_candidates = self.embedding_store.similarity_search(&query_embedding, top_k);
        
        for (chunk, semantic_score) in semantic_candidates {
            // Skip if already in BM25 results
            if hybrid_results.iter().any(|r| r.chunk.id == chunk.id) {
                continue;
            }
            
            let norm_semantic = (semantic_score + 1.0) / 2.0;
            let combined_score = (1.0 - alpha) * norm_semantic; // No BM25 component
            
            let explanation = format!(
                "Semantic-only: {:.3} (norm: {:.3}), α={:.1}",
                semantic_score, norm_semantic, alpha
            );
            
            hybrid_results.push(SearchResult {
                chunk,
                bm25_score: 0.0,
                semantic_score,
                combined_score,
                explanation,
            });
        }
        
        // Step 5: Sort by combined score and take top-k
        hybrid_results.sort_by(|a, b| b.combined_score.partial_cmp(&a.combined_score).unwrap());
        hybrid_results.truncate(top_k);
        
        // Log results for debugging
        for (i, result) in hybrid_results.iter().enumerate() {
            debug!("Result {}: {:.3} - {} ({})", 
                   i + 1, result.combined_score, result.chunk.id, result.explanation);
        }
        
        info!("✅ Hybrid search returned {} results", hybrid_results.len());
        Ok(hybrid_results)
    }
    
    fn pure_bm25_search(&self, query: &str, top_k: usize) -> Result<Vec<SearchResult>> {
        info!("Using pure BM25 search");
        let chunks = crate::retriever::bm25_search(&self.bm25_index, query, top_k)?;
        
        Ok(chunks.into_iter().map(|chunk| SearchResult {
            chunk: EnhancedChunk::from(chunk),
            bm25_score: 1.0,
            semantic_score: 0.0,
            combined_score: 1.0,
            explanation: "Pure BM25".to_string(),
        }).collect())
    }
    
    pub fn pure_semantic_search(&self, query: &str, top_k: usize) -> Result<Vec<SearchResult>> {
        info!("Using pure semantic search");
        let query_embedding = self.embedding_model.encode(query)?;
        let candidates = self.embedding_store.similarity_search(&query_embedding, top_k);
        
        Ok(candidates.into_iter().map(|(chunk, score)| SearchResult {
            chunk,
            bm25_score: 0.0,
            semantic_score: score,
            combined_score: score,
            explanation: format!("Pure semantic: {:.3}", score),
        }).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        let c = vec![0.0, 1.0, 0.0];
        
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 1e-6);
        assert!((cosine_similarity(&a, &c) - 0.0).abs() < 1e-6);
    }
    
    #[test]
    fn test_query_analysis() {
        assert!(matches!(analyze_query("sprite::new()"), SearchStrategy::BM25Heavy { .. }));
        assert!(matches!(analyze_query("how to render sprites"), SearchStrategy::SemanticHeavy { .. }));
        assert!(matches!(analyze_query("2d sprites"), SearchStrategy::SemanticHeavy { .. }));
        assert!(matches!(analyze_query("bevy animation tutorial examples"), SearchStrategy::Balanced { .. }));
    }
}