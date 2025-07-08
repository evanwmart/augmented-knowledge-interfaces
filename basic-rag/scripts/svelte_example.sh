#!/bin/bash

# svelte_example.sh
# Clone Svelte documentation and set up Basic RAG to query it

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

# Check if we're in the right directory
if [[ ! -f "Cargo.toml" ]] || [[ ! -d "src" ]]; then
    print_error "This script must be run from the basic-rag project root directory"
    print_error "Make sure you're in the directory containing Cargo.toml"
    exit 1
fi

# Store the project root directory
PROJECT_ROOT=$(pwd)
print_status "Project root: $PROJECT_ROOT"

print_status "Setting up Svelte documentation for Basic RAG..."

# Clean up any existing docs
if [[ -d "docs" ]]; then
    print_warning "Existing docs directory found. Backing up to docs.backup..."
    rm -rf docs.backup 2>/dev/null || true
    mv docs docs.backup
fi

# Clean up any existing index
if [[ -d "index" ]]; then
    print_warning "Removing existing index directory..."
    rm -rf index
fi

# Create docs directory
mkdir -p docs

# Create temporary directory for cloning
TEMP_DIR=$(mktemp -d)
print_status "Created temporary directory: $TEMP_DIR"

# Clone the full repo first, then extract what we need
print_status "Cloning Svelte repository..."
cd "$TEMP_DIR"
git clone --depth 1 https://github.com/sveltejs/svelte.git

print_success "Successfully cloned Svelte repository"

# Go back to project root
cd "$PROJECT_ROOT"

# Debug: Check what we actually got
print_status "Checking cloned structure..."
find "$TEMP_DIR/svelte" -type d -name "*doc*" | head -5 || true

# Look for documentation in various possible locations
DOC_SOURCES=()
if [[ -d "$TEMP_DIR/svelte/documentation/docs" ]]; then
    DOC_SOURCES+=("$TEMP_DIR/svelte/documentation/docs")
fi
if [[ -d "$TEMP_DIR/svelte/documentation" ]]; then
    DOC_SOURCES+=("$TEMP_DIR/svelte/documentation")
fi
if [[ -d "$TEMP_DIR/svelte/docs" ]]; then
    DOC_SOURCES+=("$TEMP_DIR/svelte/docs")
fi
if [[ -d "$TEMP_DIR/svelte/site/content/docs" ]]; then
    DOC_SOURCES+=("$TEMP_DIR/svelte/site/content/docs")
fi

# Find markdown files from any of these sources
FOUND_FILES=false
for source in "${DOC_SOURCES[@]}"; do
    if [[ -d "$source" ]]; then
        MD_COUNT=$(find "$source" -name "*.md" -type f | wc -l)
        if [[ $MD_COUNT -gt 0 ]]; then
            print_status "Found $MD_COUNT markdown files in $source"
            find "$source" -name "*.md" -type f -exec cp {} "./docs/" \; 2>/dev/null || true
            FOUND_FILES=true
        fi
    fi
done

# If we didn't find documentation in expected places, search the entire repo
if [[ "$FOUND_FILES" == false ]]; then
    print_status "Searching entire repository for markdown files..."
    find "$TEMP_DIR/svelte" -name "*.md" -type f | head -20 | while read -r file; do
        print_status "Found: $file"
        cp "$file" "./docs/" 2>/dev/null || true
    done
    FOUND_FILES=true
fi

# Clean up temporary directory
print_status "Cleaning up temporary files..."
rm -rf "$TEMP_DIR"

# Show what we got
if [[ -d "docs" ]]; then
    DOC_COUNT=$(find docs -name "*.md" -type f | wc -l)
    if [[ $DOC_COUNT -gt 0 ]]; then
        print_success "Found $DOC_COUNT markdown files in Svelte documentation"

        # List some example files
        print_status "Sample documentation files:"
        find docs -name "*.md" -type f | head -10 | while read -r file; do
            echo "  - $file"
        done

        if [[ $DOC_COUNT -gt 10 ]]; then
            echo "  ... and $((DOC_COUNT - 10)) more files"
        fi
    else
        print_error "No markdown files were copied to docs directory"
        exit 1
    fi
else
    print_error "docs directory was not created properly"
    exit 1
fi

echo ""
print_status "Building the search index..."

# Check if OpenAI API key is set
if [[ -z "${OPENAI_API_KEY}" ]]; then
    print_warning "OPENAI_API_KEY environment variable is not set"
    print_warning "You'll need to set it before running queries:"
    echo "  export OPENAI_API_KEY=\"your-api-key-here\""
    echo ""
fi

# Build the index
if cargo run -- init; then
    print_success "Index built successfully!"
else
    print_error "Failed to build index"
    exit 1
fi

echo ""
print_success "Svelte documentation setup complete!"
echo ""
print_status "You can now query the Svelte documentation with commands like:"
echo "  cargo run -- query \"How do I create a component?\""
echo "  cargo run -- query \"What is reactive programming in Svelte?\""
echo "  cargo run -- query \"How do I handle events?\""
echo "  cargo run -- query \"What are Svelte stores?\""
echo ""
print_status "To explore the documentation structure:"
echo "  find docs -name \"*.md\" | head -20"
echo "  ls -la docs/"
echo ""

# Show index statistics
if [[ -f "index/state.json" ]]; then
    CHUNK_COUNT=$(grep -o '"[^"]*":' index/state.json | wc -l)
    print_status "Index contains approximately $CHUNK_COUNT chunks"
fi

print_success "Setup complete! Happy querying! 🚀"