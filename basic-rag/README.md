# Basic RAG: Technical Specification

---

## 1. Introduction & Use Cases

**Goal:**
Build a Rust-based Retrieval-Augmented Generation (RAG) CLI that can index and query documentation using semantic search and LLM-powered answers. The system provides accurate, context-grounded responses by retrieving relevant document chunks and using them to augment LLM prompts.

**Primary Use Cases:**

1. **Interactive Documentation Q&A**: Developers run `basic-rag query "<question>"` against indexed documentation for instant answers.
2. **Documentation Knowledge Base**: Build searchable knowledge bases from Markdown documentation with source attribution.
3. **Context-Aware AI Assistance**: Get LLM responses grounded in specific documentation rather than general knowledge.

---

## 2. Core Features & Requirements

| Feature                      | Description                                                                                     | Requirements                                                                                       |
| ---------------------------- | ----------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| **Document Ingestion**       | Walk docs directory, parse Markdown files, split into overlapping semantic chunks              | - `walkdir`, custom chunking with token-aware splitting                                           |
| **Tantivy Indexing**         | Build full-text search index with BM25 scoring and incremental updates                        | - `tantivy` schema: `id`, `text`, `source`, `heading`, `position`                                 |
| **BM25 Retrieval**           | Fast keyword search retrieves top-K most relevant chunks                                       | - `QueryParser` with stemming, configurable `top_k`                                               |
| **Smart Prompt Assembly**    | Format retrieved chunks + question into optimized LLM prompts with token management           | - Multiple template styles, source attribution, token budget management                           |
| **OpenAI Integration**       | Call OpenAI Chat Completions API for high-quality answers                                      | - `reqwest` async client, proper error handling, response parsing                                 |
| **CLI Interface**            | Simple subcommands for indexing and querying via `clap`                                        | - `init` and `query` subcommands with comprehensive options                                       |
| **Incremental Updates**      | Track document changes and only reindex modified content                                       | - State tracking with content hashing, efficient diff detection                                   |
| **Logging & Error Handling** | Structured logging and clear error messages throughout                                         | - `log` crate with configurable levels                                                            |

---

## 3. Architecture Overview

```text
┌─────────────────────────────────────────────────────────────────┐
│                       basic-rag CLI                            │
│                    (clap argument parsing)                     │
└──────────────┬────────────────────────┬─────────────────────────┘
               │                        │
          init subcommand          query subcommand
               │                        │
       ┌───────▼────────┐       ┌───────▼────────┐
       │ Document       │       │ Query Pipeline │
       │ Ingestion      │       │                │
       └───────┬────────┘       └───┬────────────┘
               │                    │
   ┌───────────▼─────────┐         │       ┌─────────────────┐
   │ ingest.rs           │         │       │ retriever.rs    │
   │ - Parse Markdown    │         │       │ - BM25 search   │
   │ - Create chunks     │         │       │ - Rank results  │
   │ - Extract metadata  │         │       └─────────┬───────┘
   └───────────┬─────────┘         │                 │
               │                   │                 ▼
       ┌───────▼──────────┐        │         ┌───────────────┐
       │ indexer.rs       │        │         │ prompt.rs     │
       │ - Build/update   │        │         │ - Assemble    │
       │   Tantivy index  │        │         │   context     │
       │ - State tracking │        │         │ - Format LLM  │
       │ - Change detection│       │         │   prompt      │
       └──────────────────┘        │         └───────┬───────┘
                                   │                 │
                                   │                 ▼
                                   │         ┌───────────────┐
                                   │         │ llm.rs        │
                                   └─────────│ - OpenAI API  │
                                             │ - Response    │
                                             │   parsing     │
                                             └───────┬───────┘
                                                     │
                                                     ▼
                                             Generated Answer
                                               with Sources
```

---

## 4. File Structure & Module Specifications

```
basic-rag/
├── Cargo.toml
├── README.md
├── .env                     # OPENAI_API_KEY
├── docs/                    # Default docs directory
├── index/                   # Default index directory
├── src/
│   ├── main.rs              # CLI entrypoint and command dispatch
│   ├── cli.rs               # Clap argument parsing and CLI structure
│   ├── config.rs            # Configuration management (currently comprehensive but unused)
│   ├── ingest.rs            # Document parsing and chunking
│   ├── indexer.rs           # Tantivy index building and management
│   ├── retriever.rs         # BM25 search and ranking
│   ├── prompt.rs            # LLM prompt assembly and formatting
│   └── llm.rs               # OpenAI API integration
└── tests/
    └── integration/         # End-to-end CLI tests
```

### Module Responsibilities

#### **`main.rs`**
- CLI application entrypoint
- Command dispatch to init/query handlers
- Error handling and logging setup

#### **`cli.rs`** 
- **Primary CLI structure using `clap`**
- Command-line argument parsing and validation
- Defines subcommands: `Init`, `Query`
- Configuration options:
  - `--docs-dir`: Source documentation directory
  - `--index-dir`: Search index storage location  
  - `--chunk-size`/`--chunk-overlap`: Text chunking parameters
  - `--top-k`: Number of search results to retrieve
  - `--openai-api-key`: OpenAI API authentication

#### **`ingest.rs`**
- **Document processing pipeline**
- Walk directory tree and discover Markdown files
- Parse Markdown content and extract text
- Split documents into overlapping semantic chunks
- Extract metadata (headings, file paths, positions)
- Return structured `Chunk` objects for indexing

#### **`indexer.rs`**
- **Tantivy full-text search index management**
- Build new indexes from document chunks
- Incremental updates with change detection
- Index state tracking and persistence
- Schema definition for search fields
- Efficient batch indexing operations

#### **`retriever.rs`**
- **BM25 search implementation**
- Parse user queries and build search terms
- Execute searches against Tantivy index
- Rank and return top-K most relevant chunks
- Convert search results back to `Chunk` objects

#### **`prompt.rs`**
- **LLM prompt engineering and assembly**
- Format retrieved chunks into coherent context
- Multiple prompt template styles (Chat, Completion, Conversational)
- Token budget management and text truncation
- Source attribution and metadata inclusion
- Configurable prompt behavior

#### **`llm.rs`**
- **OpenAI API integration**
- Async HTTP client for Chat Completions API
- Request/response serialization and parsing
- Error handling and retry logic
- Response validation and extraction

#### **`config.rs`**
- **Comprehensive configuration system** (currently unused)
- Server, database, logging, authentication settings
- Environment variable overrides
- Configuration file support (JSON)
- Validation and error handling

---

## 5. Usage Examples

### Initialize Index
```bash
# Index documentation in ./docs directory
basic-rag init

# Index custom documentation path
basic-rag --docs-dir /path/to/docs --index-dir ./my-index init
```

### Query Documentation
```bash
# Ask a question
basic-rag query "How do I configure logging?"

# Use custom parameters
basic-rag --top-k 10 query "What are the authentication options?"
```

### Environment Configuration
```bash
# Set OpenAI API key
export OPENAI_API_KEY="your-api-key-here"

# Or pass as argument
basic-rag --openai-api-key "your-key" query "How does indexing work?"
```

---

## 6. Key Implementation Details

### Document Chunking Strategy
- Token-aware splitting (default 500 tokens with 50 token overlap)
- Preserve paragraph and section boundaries when possible
- Include heading context for better semantic understanding
- Unique chunk IDs for change detection and updates

### Search & Retrieval
- BM25 ranking with English stemming
- Full-text search across chunk content
- Configurable result limits (default top-5)
- Source file and position tracking for attribution

### Prompt Engineering
- Multiple template styles for different LLM APIs
- Token budget management to fit context windows
- Automatic text truncation with boundary preservation
- Source attribution for fact checking and follow-up

### Incremental Updates
- Content-based change detection using hashing
- State persistence in JSON format
- Only reindex modified or new documents
- Efficient batch operations for large document sets

---

## 7. Non-Functional Requirements

* **Performance**: Sub-100ms search queries for ~1K document chunks
* **Scalability**: Handle thousands of documents with incremental indexing
* **Reliability**: Robust error handling and graceful degradation
* **Maintainability**: Clear module separation and comprehensive testing
* **Usability**: Simple CLI interface with sensible defaults
* **Portability**: Cross-platform support (Linux/macOS/Windows)




# Setting Up Hybrid Search with Embeddings

## Prerequisites

1. **Install Python Dependencies**:
   ```bash
   pip install sentence-transformers numpy
   ```

2. **Ensure Python 3 is available**:
   ```bash
   python3 --version
   ```

## Usage

### Building an Index with Embeddings

```bash
# Initialize the index with embeddings (default)
cargo run -- init

# Or explicitly build without embeddings (BM25 only)
cargo run -- init --skip-embeddings
```

### Querying with Different Strategies

```bash
# Auto-detect best strategy based on query
cargo run -- query "how to implement authentication"

# Force pure BM25 search
cargo run -- query "UserAuth::new()" --strategy bm25

# Force pure semantic search
cargo run -- query "security best practices" --strategy semantic

# Force hybrid search with custom weighting
cargo run -- query "implement oauth2 in rust" --strategy hybrid --alpha 0.7
```

### Search Strategy Guidelines

- **BM25** (`--strategy bm25`): Best for exact matches, API names, function signatures
- **Semantic** (`--strategy semantic`): Best for conceptual queries, "how to" questions
- **Hybrid** (`--strategy hybrid`): Balanced approach, good for most queries
- **Auto** (default): Let the system analyze your query and choose

### Alpha Values for Hybrid Search

The `--alpha` parameter controls the balance between BM25 and semantic search:
- `1.0` = 100% BM25, 0% semantic
- `0.7` = 70% BM25, 30% semantic
- `0.5` = 50% BM25, 50% semantic (balanced)
- `0.3` = 30% BM25, 70% semantic
- `0.0` = 0% BM25, 100% semantic

## Troubleshooting

### "Failed to execute Python embedding script"
- Ensure `sentence-transformers` is installed: `pip install sentence-transformers`
- Check Python 3 is in your PATH: `which python3`

### "Embeddings not found!"
- Run `cargo run -- init` to build the index with embeddings
- Check that `index/embeddings.json` exists

### Slow embedding generation
- First run downloads the model (~90MB)
- Subsequent runs will be faster
- Consider using `--skip-embeddings` for quick testing

## Implementation Notes

The hybrid search system:
1. Uses `all-MiniLM-L6-v2` model (384 dimensions)
2. Creates embeddings for chunk text + heading
3. Stores embeddings in `index/embeddings.json`
4. Combines BM25 and cosine similarity scores
5. Automatically analyzes queries to choose optimal strategy