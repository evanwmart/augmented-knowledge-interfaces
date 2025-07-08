mod cli;
mod config;
mod ingest;
mod indexer;
mod prompt;
mod retriever;
mod llm;
mod embeddings;

use anyhow::Result;
use crate::cli::{Cli, Command, SearchStrategy};
use clap::Parser;
use dotenv::dotenv;
use env_logger::init as logger_init;
use log::info;

#[tokio::main]
async fn main() -> Result<()> {
    // 1) Load .env and initialize logger
    dotenv().ok();
    logger_init();
    
    // 2) Parse CLI args
    let cli = Cli::parse();
    info!("Invoked command: {:?}", cli.command);
    
    match cli.command {
        Command::Init { skip_embeddings } => {
            info!("🛠️ Initializing docs & index…");
            
            // 1) Sync docs folder (clone or pull)
            ingest::sync_docs(&cli.docs_dir)?;
            
            // 2) Read & chunk
            let chunks = ingest::ingest_docs(&cli)?;
            
            if skip_embeddings {
                // 3a) Build traditional BM25-only index
                indexer::build_index(&cli, &chunks)?;
                info!("✅ BM25 index built at `{}`", cli.index_dir.display());
            } else {
                // 3b) Build enhanced index with embeddings
                embeddings::build_enhanced_index(&cli, &chunks).await?;
                info!("✅ Enhanced index with embeddings built at `{}`", cli.index_dir.display());
            }
        }
        Command::Query { ref query, ref strategy, alpha } => {
            info!("🔍 Opening index at `{}`…", cli.index_dir.display());
            
            // Check if embeddings are available
            let embeddings_path = cli.index_dir.join("embeddings.json");
            let has_embeddings = embeddings_path.exists();
            
            if !has_embeddings && matches!(strategy, SearchStrategy::Semantic | SearchStrategy::Hybrid) {
                anyhow::bail!("Embeddings not found! Run 'init' without --skip-embeddings first.");
            }
            
            let chunks = if has_embeddings && !matches!(strategy, SearchStrategy::Bm25) {
                // Use hybrid search
                info!("Using hybrid search with strategy: {:?}", strategy);
                let hybrid_searcher = embeddings::HybridSearcher::new(&cli)?;
                
                let search_results = match strategy {
                    SearchStrategy::Auto => {
                        // Let the system decide based on query analysis
                        hybrid_searcher.hybrid_search(query, cli.top_k)?
                    }
                    SearchStrategy::Semantic => {
                        // Force pure semantic search
                        let strategy = embeddings::SearchStrategy::PureSemantic;
                        match strategy {
                            embeddings::SearchStrategy::PureSemantic => 
                                hybrid_searcher.pure_semantic_search(query, cli.top_k)?,
                            _ => unreachable!(),
                        }
                    }
                    SearchStrategy::Hybrid => {
                        // Force hybrid with custom alpha or default
                        let alpha = alpha.unwrap_or(0.5);
                        hybrid_searcher.hybrid_search_with_alpha(query, cli.top_k, alpha)?
                    }
                    SearchStrategy::Bm25 => {
                        // This branch shouldn't be reached due to the outer check
                        unreachable!()
                    }
                };
                
                // Log search results
                for (i, result) in search_results.iter().enumerate() {
                    info!("Result {}: {} (score: {:.3}, {})", 
                          i + 1, 
                          result.chunk.id, 
                          result.combined_score,
                          result.explanation);
                }
                
                // Convert to regular chunks
                search_results.into_iter()
                    .map(|result| crate::ingest::Chunk::from(&result.chunk))
                    .collect()
            } else {
                // Use traditional BM25 search
                info!("Using traditional BM25 search");
                let indexer_index = indexer::open_index(&cli)?;
                let retriever_index = retriever::Index::new(indexer_index.tantivy_index)?;
                retriever::bm25_search(&retriever_index, query, cli.top_k)?
            };
            
            // 2) Assemble prompt
            let prompt = prompt::build_prompt(&chunks, query);
            
            // 3) Call LLM
            let answer = llm::query_llm(&cli.openai_api_key, &prompt).await?;
            
            // 4) Print result
            println!("\n{}", answer);
        }
    }
    
    Ok(())
}