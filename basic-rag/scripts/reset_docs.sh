#!/bin/bash

# reset_docs.sh
# Reset the docs directory and index for Basic RAG
# Useful for cleaning up and starting fresh with different documentation

set -e  # Exit on any error

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Function to print colored output
print_status() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

print_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

print_header() {
    echo ""
    echo -e "${BLUE}================================${NC}"
    echo -e "${BLUE}  Basic RAG Documentation Reset${NC}"
    echo -e "${BLUE}================================${NC}"
    echo ""
}

# Check if we're in the right directory
if [[ ! -f "Cargo.toml" ]] || [[ ! -d "src" ]]; then
    print_error "This script must be run from the basic-rag project root directory"
    print_error "Make sure you're in the directory containing Cargo.toml"
    exit 1
fi

print_header

# Parse command line arguments
FORCE=false
BACKUP=true
CREATE_SAMPLE=false

while [[ $# -gt 0 ]]; do
    case $1 in
        -f|--force)
            FORCE=true
            shift
            ;;
        --no-backup)
            BACKUP=false
            shift
            ;;
        -s|--sample)
            CREATE_SAMPLE=true
            shift
            ;;
        -h|--help)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Reset the docs directory and search index for Basic RAG"
            echo ""
            echo "Options:"
            echo "  -f, --force      Skip confirmation prompt"
            echo "  --no-backup      Don't create backup of existing docs"
            echo "  -s, --sample     Create sample documentation after reset"
            echo "  -h, --help       Show this help message"
            echo ""
            echo "Examples:"
            echo "  $0                    # Interactive reset with backup"
            echo "  $0 --force --sample  # Force reset and create sample docs"
            echo "  $0 --no-backup -f    # Force reset without backup"
            exit 0
            ;;
        *)
            print_error "Unknown option: $1"
            echo "Use -h or --help for usage information"
            exit 1
            ;;
    esac
done

# Show current state
print_status "Current state:"
if [[ -d "docs" ]]; then
    DOC_COUNT=$(find docs -name "*.md" -type f 2>/dev/null | wc -l)
    DOC_SIZE=$(du -sh docs 2>/dev/null | cut -f1)
    echo "  📁 docs/ directory exists with $DOC_COUNT markdown files ($DOC_SIZE)"
else
    echo "  📁 docs/ directory does not exist"
fi

if [[ -d "index" ]]; then
    INDEX_SIZE=$(du -sh index 2>/dev/null | cut -f1)
    echo "  🔍 index/ directory exists ($INDEX_SIZE)"
    if [[ -f "index/state.json" ]]; then
        CHUNK_COUNT=$(grep -o '"[^"]*":' index/state.json 2>/dev/null | wc -l)
        echo "  📊 Index contains approximately $CHUNK_COUNT chunks"
    fi
else
    echo "  🔍 index/ directory does not exist"
fi

if [[ -f "state.json" ]]; then
    echo "  📄 state.json file exists in root"
fi

echo ""

# Confirmation prompt unless --force is used
if [[ "$FORCE" != true ]]; then
    echo -e "${YELLOW}This will remove:${NC}"
    [[ -d "docs" ]] && echo "  - docs/ directory"
    [[ -d "index" ]] && echo "  - index/ directory" 
    [[ -f "state.json" ]] && echo "  - state.json file"
    echo ""
    
    if [[ "$BACKUP" == true ]] && [[ -d "docs" ]]; then
        echo -e "${GREEN}A backup will be created as docs.backup${NC}"
        echo ""
    fi
    
    read -p "Are you sure you want to continue? (y/N): " -n 1 -r
    echo ""
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        print_status "Reset cancelled"
        exit 0
    fi
fi

print_status "Starting reset process..."

# Backup existing docs if requested and they exist
if [[ "$BACKUP" == true ]] && [[ -d "docs" ]]; then
    print_status "Creating backup of existing docs..."
    
    # Remove old backup if it exists
    if [[ -d "docs.backup" ]]; then
        rm -rf docs.backup
    fi
    
    cp -r docs docs.backup
    print_success "Backup created at docs.backup/"
fi

# Remove docs directory
if [[ -d "docs" ]]; then
    print_status "Removing docs/ directory..."
    rm -rf docs
    print_success "docs/ directory removed"
fi

# Remove index directory
if [[ -d "index" ]]; then
    print_status "Removing index/ directory..."
    rm -rf index
    print_success "index/ directory removed"
fi

# Remove root state.json if it exists
if [[ -f "state.json" ]]; then
    print_status "Removing state.json file..."
    rm -f state.json
    print_success "state.json file removed"
fi

# Create sample documentation if requested
if [[ "$CREATE_SAMPLE" == true ]]; then
    print_status "Creating sample documentation..."
    
    mkdir -p docs
    
    cat > docs/README.md << 'EOF'
# Basic RAG Documentation

Welcome to the Basic RAG documentation! This is a sample documentation set for testing the RAG system.

## What is Basic RAG?

Basic RAG is a Rust-based Retrieval-Augmented Generation CLI that helps you build searchable knowledge bases from documentation.

## Key Features

- **Document Indexing**: Automatically parses and indexes Markdown documentation
- **Semantic Search**: Uses BM25 scoring for fast and relevant search results
- **AI-Powered Answers**: Integrates with OpenAI to provide context-aware responses
- **Incremental Updates**: Only reprocesses changed documents for efficiency
EOF

    cat > docs/installation.md << 'EOF'
# Installation Guide

## Prerequisites

Before installing Basic RAG, ensure you have:

1. **Rust and Cargo**: Install from [rustup.rs](https://rustup.rs/)
2. **OpenAI API Key**: Get one from [OpenAI Platform](https://platform.openai.com/)

## Installation Steps

1. Clone the repository:
   ```bash
   git clone <repository-url>
   cd basic-rag
   ```

2. Build the project:
   ```bash
   cargo build --release
   ```

3. Set your OpenAI API key:
   ```bash
   export OPENAI_API_KEY="your-api-key-here"
   ```

## Verification

Test the installation:
```bash
cargo run -- --help
```

You should see the CLI help message with available commands and options.
EOF

    cat > docs/usage.md << 'EOF'
# Usage Guide

## Basic Workflow

The typical workflow involves two steps:

1. **Initialize the index** with your documentation
2. **Query the documentation** using natural language

## Indexing Documentation

To index your documentation:

```bash
# Index the default docs/ directory
cargo run -- init

# Index a custom directory
cargo run -- --docs-dir /path/to/docs init
```

## Querying Documentation

Ask questions about your documentation:

```bash
# Ask a question
cargo run -- query "How do I install the system?"

# Get more results
cargo run -- --top-k 10 query "What are the configuration options?"
```

## Configuration Options

- `--docs-dir`: Source documentation directory (default: ./docs)
- `--index-dir`: Search index location (default: ./index)
- `--chunk-size`: Text chunk size in tokens (default: 500)
- `--chunk-overlap`: Overlap between chunks (default: 50)
- `--top-k`: Number of search results (default: 5)
EOF

    cat > docs/troubleshooting.md << 'EOF'
# Troubleshooting

## Common Issues

### Build Errors

If you encounter build errors:

1. **Update Rust**: Ensure you have the latest stable Rust version
   ```bash
   rustup update stable
   ```

2. **Clean build**: Remove build artifacts and rebuild
   ```bash
   cargo clean
   cargo build
   ```

3. **Check dependencies**: Verify all dependencies are available

### Index Issues

If indexing fails:

- **Check permissions**: Ensure you can read the docs directory
- **Verify file format**: Only Markdown (.md) files are supported
- **Check disk space**: Indexing requires temporary storage

### Query Issues

If queries don't work:

- **API Key**: Verify your OpenAI API key is set correctly
- **Network**: Check your internet connection
- **Index**: Ensure the index was built successfully

### Performance Issues

If the system is slow:

- **Reduce chunk size**: Use smaller chunks for faster processing
- **Limit results**: Use lower --top-k values
- **Check system resources**: Ensure adequate RAM and CPU

## Getting Help

If you continue to have issues:

1. Check the logs with `RUST_LOG=debug`
2. Verify your setup with the sample documentation
3. Review the configuration options
EOF

    DOC_COUNT=$(find docs -name "*.md" -type f | wc -l)
    print_success "Created $DOC_COUNT sample documentation files"
    
    print_status "Sample files created:"
    find docs -name "*.md" -type f | while read -r file; do
        echo "  - $file"
    done
fi

echo ""
print_success "Reset complete!"

# Show next steps
echo ""
print_status "Next steps:"

if [[ "$CREATE_SAMPLE" == true ]]; then
    echo "  1. Build the index: cargo run -- init"
    echo "  2. Try a query: cargo run -- query \"How do I install Basic RAG?\""
else
    echo "  1. Add your documentation to the docs/ directory"
    echo "  2. Build the index: cargo run -- init"
    echo "  3. Start querying: cargo run -- query \"your question\""
fi

echo ""
print_status "Available scripts:"
echo "  - ./scripts/svelte_example.sh    # Set up with Svelte documentation"
echo "  - ./scripts/reset_docs.sh -s     # Reset and create sample docs"

if [[ "$BACKUP" == true ]] && [[ -d "docs.backup" ]]; then
    echo ""
    print_status "Your previous documentation is backed up in docs.backup/"
    echo "  To restore: mv docs.backup docs"
fi

echo ""
print_success "Happy documenting! 📚"