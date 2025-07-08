use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "basic-rag")]
#[command(about = "A basic RAG system with hybrid search", long_about = None)]
pub struct Cli {
    /// Directory containing documents to index
    #[arg(short, long, default_value = "./docs")]
    pub docs_dir: PathBuf,
    
    /// Directory to store the search index
    #[arg(short, long, default_value = "./index")]
    pub index_dir: PathBuf,
    
    /// Chunk size in tokens
    #[arg(long, default_value = "500")]
    pub chunk_size: usize,
    
    /// Chunk overlap in tokens
    #[arg(long, default_value = "50")]
    pub chunk_overlap: usize,
    
    /// Number of top results to retrieve
    #[arg(short = 'k', long, default_value = "5")]
    pub top_k: usize,
    
    /// OpenAI API key
    #[arg(long, env = "OPENAI_API_KEY")]
    pub openai_api_key: String,
    
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Initialize the index with embeddings
    Init {
        /// Skip embedding generation (BM25 only)
        #[arg(long)]
        skip_embeddings: bool,
    },
    
    /// Query the index
    Query {
        /// The query string
        query: String,
        
        /// Force specific search strategy: bm25, semantic, or hybrid (default: auto)
        #[arg(long, default_value = "auto")]
        strategy: SearchStrategy,
        
        /// Custom alpha value for hybrid search (0.0-1.0, where 1.0 = pure BM25)
        #[arg(long)]
        alpha: Option<f32>,
    },
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum SearchStrategy {
    /// Automatically choose strategy based on query analysis
    Auto,
    /// Use only BM25 search
    Bm25,
    /// Use only semantic search
    Semantic,
    /// Use hybrid search with balanced weights
    Hybrid,
}