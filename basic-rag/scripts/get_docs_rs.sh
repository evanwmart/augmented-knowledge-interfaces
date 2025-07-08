#!/bin/bash

# cargo_docs_scraper.sh
# Scrape documentation for any cargo library from docs.rs
# Usage: ./cargo_docs_scraper.sh <library_name>

set -e  # Exit on any error

# Check if library name is provided
if [[ -z "$1" ]]; then
    echo "Usage: $0 <library_name>"
    echo "Example: $0 bevy"
    exit 1
fi

LIBRARY_NAME="$1"

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

# Function to flatten directory structure
flatten_docs() {
    local docs_dir="$1"
    print_status "Flattening directory structure..."
    
    # Create a temporary directory for the flattened files
    local temp_flat_dir=$(mktemp -d)
    
    # Find all .md files and copy them to the temp directory with prefixed names
    find "$docs_dir" -name "*.md" -type f | while read -r file; do
        # Get the relative path from docs_dir
        relative_path=$(realpath --relative-to="$docs_dir" "$file")
        
        # Convert path separators to underscores and remove .md extension
        flattened_name=$(echo "$relative_path" | sed 's|/|_|g')
        
        # Copy file to temp directory
        cp "$file" "$temp_flat_dir/$flattened_name"
        echo "  Flattened: $relative_path -> $flattened_name"
    done
    
    # Remove all subdirectories and move flattened files back
    find "$docs_dir" -mindepth 1 -type d -exec rm -rf {}  2>/dev/null || true
    
    # Move flattened files back to docs directory
    mv "$temp_flat_dir"/* "$docs_dir/" 2>/dev/null || true

    # Remove any remaining subdirectories so only .md files remain at the top level
    print_status "Cleaning up leftover directories..."
    find "$docs_dir" -mindepth 1 -type d -exec rm -rf {} + 2>/dev/null || true
+
    
    # Clean up temp directory
    rm -rf "$temp_flat_dir"
    
    local flattened_count=$(find "$docs_dir" -name "*.md" -type f | wc -l)
    print_success "Flattened $flattened_count files into $docs_dir/"
}

# Setup Python virtual environment and dependencies
setup_python_env() {
    local venv_dir="$TEMP_DIR/venv"
    
    # Check if python3 is available
    if ! command -v python3 &> /dev/null; then
        print_error "python3 is required but not installed" >&2
        print_status "Please install Python 3:" >&2
        echo "  # On Ubuntu/Debian: sudo apt update && sudo apt install python3 python3-venv" >&2
        echo "  # On Fedora/RHEL: sudo dnf install python3" >&2
        echo "  # On macOS: brew install python3" >&2
        exit 1
    fi
    
    print_status "Creating Python virtual environment..." >&2
    python3 -m venv "$venv_dir" >&2
    
    print_status "Activating virtual environment and installing dependencies..." >&2
    source "$venv_dir/bin/activate"
    
    # Upgrade pip first
    pip install --upgrade pip > /dev/null 2>&1
    
    # Install required packages
    pip install requests beautifulsoup4 html2text > /dev/null 2>&1
    
    print_success "Python environment setup complete" >&2
    
    # Return the python path for later use (this is the only stdout output)
    echo "$venv_dir/bin/python"
}

# Store the project root directory
PROJECT_ROOT=$(pwd)
print_status "Project root: $PROJECT_ROOT"

print_status "Setting up documentation for $LIBRARY_NAME from docs.rs..."

# Create temporary directory for scraping
TEMP_DIR=$(mktemp -d)
print_status "Created temporary directory: $TEMP_DIR"

# Setup Python environment and get python path
PYTHON_BIN=$(setup_python_env)

# Clean up any existing docs for this library
DOCS_DIR="docs"
if [[ -d "$DOCS_DIR" ]]; then
    print_warning "Existing $DOCS_DIR directory found. Backing up to ${DOCS_DIR}.backup..."
    rm -rf "${DOCS_DIR}.backup" 2>/dev/null || true
    mv "$DOCS_DIR" "${DOCS_DIR}.backup"
fi

# Create docs directory
mkdir -p "$DOCS_DIR"

# Create Python scraper script
cat > "$TEMP_DIR/scrape_cargo_docs.py" << 'EOF'
#!/usr/bin/env python3

import requests
from bs4 import BeautifulSoup
import html2text
import os
import sys
import time
import urllib.parse
from pathlib import Path
import re
from collections import deque

class CargoDocsScraper:
    def __init__(self, library_name, output_dir="docs"):
        self.library_name = library_name
        self.base_url = f"https://docs.rs/{library_name}/latest/{library_name}/"
        self.output_dir = Path(output_dir)
        self.output_dir.mkdir(exist_ok=True)
        self.visited_urls = set()
        self.session = requests.Session()
        self.session.headers.update({
            'User-Agent': 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36'
        })
        self.html_converter = html2text.HTML2Text()
        self.html_converter.ignore_links = False
        self.html_converter.ignore_images = False
        self.html_converter.body_width = 0
        self.total_scraped = 0
        
    def clean_filename(self, name):
        """Clean filename for filesystem compatibility"""
        # Remove or replace problematic characters
        name = re.sub(r'[<>:"/\\|?*]', '_', name)
        name = re.sub(r'[^\w\s-]', '', name)
        name = re.sub(r'[-\s]+', '-', name)
        return name.strip('-_')[:100]  # Limit length
    
    def get_page_content(self, url):
        """Fetch and parse a page"""
        try:
            print(f"Fetching: {url}")
            response = self.session.get(url, timeout=15)
            response.raise_for_status()
            return BeautifulSoup(response.content, 'html.parser')
        except Exception as e:
            print(f"Error fetching {url}: {e}")
            return None
    
    def extract_main_content(self, soup):
        """Extract the main documentation content"""
        # Try different selectors for docs.rs content
        content_selectors = [
            'main.content',
            '.docblock',
            '#main-content',
            '.rustdoc',
            'main'
        ]
        
        for selector in content_selectors:
            content = soup.select_one(selector)
            if content:
                return content
        
        return soup
    
    def is_valid_docs_url(self, url):
        """Check if URL is part of library documentation we want"""
        if not (f'docs.rs/{self.library_name}' in url and 
                not url.endswith('.js') and
                not url.endswith('.css') and
                not url.startswith('javascript:')):
            return False
        
        # Exclude source code and fragment links
        excluded_patterns = [
            '/src/',      # Source code links
            '#',          # Fragment links
        ]
        
        # Check exclusions
        for pattern in excluded_patterns:
            if pattern in url:
                return False
        
        # Only include URLs that are part of the main library documentation
        if f'/{self.library_name}/latest/{self.library_name}/' in url:
            return True
            
        return False
    
    def extract_modules_from_main_page(self, soup):
        """Extract module names specifically from the main library page"""
        modules = []
        
        # Look for the modules section - docs.rs usually has a specific structure
        # First try to find all h3 elements that might be module names
        for h3 in soup.find_all('h3'):
            a_tag = h3.find('a')
            if a_tag and a_tag.get('href'):
                href = a_tag.get('href', '')
                text = a_tag.get_text(strip=True)
                # Module links on main page are usually like "module_name/index.html"
                if '/' in href and not any(x in href for x in ['/struct.', '/enum.', '/trait.', '/fn.', '/type.', '/macro.']):
                    modules.append((href, text))
        
        # Also look for links in item tables that might be modules
        for table in soup.find_all(['table', 'div'], class_=['item-table', 'module-item']):
            for link in table.find_all('a', href=True):
                href = link.get('href', '')
                text = link.get_text(strip=True)
                if '/' in href and not any(x in href for x in ['/struct.', '/enum.', '/trait.', '/fn.', '/type.', '/macro.', 'http', '#']):
                    modules.append((href, text))
        
        # Look for any link that appears to be a module (ends with / or /index.html)
        for link in soup.find_all('a', href=True):
            href = link.get('href', '')
            text = link.get_text(strip=True)
            parent_text = link.parent.get_text(strip=True) if link.parent else ''
            
            # Check if this looks like a module link
            if (href and text and 
                not href.startswith('http') and 
                not href.startswith('#') and
                '/' in href and
                not any(x in href for x in ['/struct.', '/enum.', '/trait.', '/fn.', '/type.', '/macro.', '../'])):
                # Additional check: module names are usually just the name, not descriptions
                if len(text.split()) <= 3 and text.lower() not in ['rust', 'docs.rs', 'platform', 'source']:
                    modules.append((href, text))
        
        # Deduplicate
        seen = set()
        unique_modules = []
        for href, text in modules:
            if href not in seen:
                seen.add(href)
                unique_modules.append((href, text))
        
        return unique_modules
    
    def get_module_links(self, soup, current_url):
        """Extract links to modules from a module index page"""
        # For the main page, use special extraction
        if current_url.rstrip('/').endswith(f'/{self.library_name}') or current_url.endswith(f'/{self.library_name}/index.html'):
            modules = self.extract_modules_from_main_page(soup)
            
            # Convert to full URLs
            full_urls = []
            for href, text in modules:
                if not href.startswith('http'):
                    full_url = urllib.parse.urljoin(current_url, href)
                else:
                    full_url = href
                
                if self.is_valid_docs_url(full_url):
                    full_urls.append((full_url, text))
            
            return full_urls
        
        # For other pages, look for sub-modules
        links = []
        for link in soup.find_all('a', href=True):
            href = link.get('href', '').strip()
            if ('index.html' in href or href.endswith('/')) and not any(x in href for x in ['/struct.', '/enum.', '/trait.', '/fn.', '/type.', '/macro.']):
                text = link.get_text(strip=True)
                
                if not href.startswith('http'):
                    full_url = urllib.parse.urljoin(current_url, href)
                else:
                    full_url = href
                    
                if self.is_valid_docs_url(full_url) and full_url not in self.visited_urls:
                    links.append((full_url, text))
        
        return links
    
    def get_item_links(self, soup, current_url):
        """Extract links to structs, enums, traits, etc. from a module page"""
        links = []
        
        # Look for all links that contain item type patterns
        for link in soup.find_all('a', href=True):
            href = link.get('href', '')
            if any(pattern in href for pattern in ['/struct.', '/enum.', '/trait.', '/fn.', '/type.', '/macro.', '/constant.']):
                text = link.get_text(strip=True)
                
                if not href.startswith('http'):
                    full_url = urllib.parse.urljoin(current_url, href)
                else:
                    full_url = href
                
                if self.is_valid_docs_url(full_url) and full_url not in self.visited_urls:
                    links.append((full_url, text))
        
        # Deduplicate
        seen = set()
        unique_links = []
        for url, text in links:
            if url not in seen:
                seen.add(url)
                unique_links.append((url, text))
        
        return unique_links
    
    def get_page_type(self, url):
        """Determine the type of documentation page"""
        if '/struct.' in url:
            return 'struct'
        elif '/enum.' in url:
            return 'enum'
        elif '/trait.' in url:
            return 'trait'
        elif '/fn.' in url:
            return 'function'
        elif '/macro.' in url:
            return 'macro'
        elif '/constant.' in url:
            return 'constant'
        elif '/type.' in url:
            return 'type'
        elif url.endswith('/index.html') or url.endswith('/'):
            return 'module'
        else:
            return 'other'
    
    def scrape_page(self, url, link_text="", inline_items=False):
        """Scrape a single page and save as markdown"""
        if url in self.visited_urls:
            return [], []
        
        self.visited_urls.add(url)
        soup = self.get_page_content(url)
        
        if not soup:
            return [], []
        
        # Extract main content
        main_content = self.extract_main_content(soup)
        
        # Get page title
        title_elem = soup.find('title')
        title = title_elem.get_text() if title_elem else link_text or f"{self.library_name} Documentation"
        title = title.replace(' - Rust', '').strip()
        
        # Convert to markdown
        markdown_content = self.html_converter.handle(str(main_content))
        
        # Create a better title with URL context
        page_type = self.get_page_type(url)
        
        # Get links based on page type
        module_links = []
        item_links = []
        
        if page_type == 'module':
            # This is a module page, get both sub-modules and items
            module_links = self.get_module_links(soup, url)
            item_links = self.get_item_links(soup, url)
        
        # For module pages, append the content of items inline
        if page_type == 'module' and inline_items and item_links:
            markdown_content += "\n\n---\n\n## Module Contents\n\n"
            
            for item_url, item_name in item_links:
                if item_url not in self.visited_urls:
                    print(f"  Fetching inline: {item_name}")
                    self.visited_urls.add(item_url)
                    item_soup = self.get_page_content(item_url)
                    
                    if item_soup:
                        item_content = self.extract_main_content(item_soup)
                        item_markdown = self.html_converter.handle(str(item_content))
                        item_type = self.get_page_type(item_url)
                        
                        markdown_content += f"\n### {item_type.capitalize()}: {item_name}\n"
                        markdown_content += f"**Source:** {item_url}\n\n"
                        markdown_content += item_markdown
                        markdown_content += "\n\n---\n\n"
                        
                        time.sleep(0.2)  # Small delay between requests
        
        # Clean up the markdown and add metadata
        final_markdown = f"""# {title}

**Page Type:** {page_type}  
**URL:** {url}

{markdown_content}
"""
        
        # Create filename based on URL
        url_path = url.replace(self.base_url, '').replace('https://docs.rs/', '')
        url_path = url_path.replace('.html', '').replace('index', '')
        
        # Create directory structure
        if '/' in url_path:
            parts = url_path.split('/')
            if len(parts) > 1:
                subdir = self.output_dir / '/'.join(parts[:-1])
                subdir.mkdir(parents=True, exist_ok=True)
                filename = self.clean_filename(parts[-1] or 'index') + '.md'
                filepath = subdir / filename
            else:
                filename = self.clean_filename(url_path) + '.md'
                filepath = self.output_dir / filename
        else:
            filename = self.clean_filename(url_path or 'index') + '.md'
            filepath = self.output_dir / filename
        
        # Save to file
        with open(filepath, 'w', encoding='utf-8') as f:
            f.write(final_markdown)
        
        print(f"Saved: {filepath.relative_to(self.output_dir)}")
        self.total_scraped += 1
        
        # Add a small delay to be respectful
        time.sleep(0.3)
        
        # If we fetched items inline, clear them from the return list
        if inline_items and page_type == 'module':
            return module_links, []
        else:
            return module_links, item_links
    
    def scrape_all(self, max_pages=500, inline_module_items=True):
        """Scrape following the pattern: main -> modules -> items in each module"""
        print(f"Starting scrape of {self.library_name} documentation")
        print(f"Base URL: {self.base_url}")
        print(f"Will scrape up to {max_pages} pages")
        if inline_module_items:
            print("Module items will be included inline in module pages")
        
        # First, scrape the main page
        main_url = self.base_url.rstrip('/') + '/index.html'
            
        module_links, item_links = self.scrape_page(main_url, f"{self.library_name} main")
        
        print(f"\nFound {len(module_links)} modules on main page")
        
        # Debug: print found module links
        if module_links:
            print("Module links found:")
            for url, name in module_links[:10]:  # Show first 10
                print(f"  - {name}: {url}")
            if len(module_links) > 10:
                print(f"  ... and {len(module_links) - 10} more")
        
        # Keep track of all modules to visit
        modules_to_visit = deque(module_links)
        all_item_links = []
        
        # Add any items found on the main page
        all_item_links.extend(item_links)
        
        # Visit each module and collect all items
        print("\nScraping modules...")
        module_count = 0
        while modules_to_visit and self.total_scraped < max_pages:
            module_url, module_name = modules_to_visit.popleft()
            module_count += 1
            
            print(f"\n[{module_count}/{len(module_links)}] Processing module: {module_name}")
            sub_module_links, item_links = self.scrape_page(module_url, module_name, inline_items=inline_module_items)
            
            # Add sub-modules to visit
            modules_to_visit.extend(sub_module_links)
            
            # Collect all items (only if not inlining)
            if not inline_module_items:
                all_item_links.extend(item_links)
            
            print(f"  Found {len(sub_module_links)} sub-modules")
            if item_links:
                print(f"  Found {len(item_links)} items (will be scraped separately)")
        
        # If not inlining, scrape all items separately
        if not inline_module_items and all_item_links:
            print(f"\nTotal items to scrape separately: {len(all_item_links)}")
            print("\nScraping items...")
            
            item_count = 0
            for item_url, item_name in all_item_links:
                if self.total_scraped >= max_pages:
                    print(f"\nReached max pages limit ({max_pages})")
                    break
                
                item_count += 1
                print(f"[{item_count}/{len(all_item_links)}] Scraping: {item_name}")
                self.scrape_page(item_url, item_name)
                
                # Progress indicator
                if self.total_scraped % 10 == 0:
                    print(f"Progress: {self.total_scraped} pages scraped")
        
        print(f"\nScraping complete! Total pages: {self.total_scraped}")

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: python scrape_cargo_docs.py <library_name> [output_dir]")
        sys.exit(1)
    
    library_name = sys.argv[1]
    output_dir = sys.argv[2] if len(sys.argv) > 2 else f"docs_{library_name}"
    
    scraper = CargoDocsScraper(library_name=library_name, output_dir=output_dir)
    scraper.scrape_all(max_pages=500, inline_module_items=True)
EOF

# Run the scraper
print_status "Running documentation scraper for $LIBRARY_NAME..."
print_status "Using Python: $PYTHON_BIN"
cd "$PROJECT_ROOT"

# Verify the Python path exists
if [[ ! -f "$PYTHON_BIN" ]]; then
    print_error "Python binary not found at: $PYTHON_BIN"
    exit 1
fi

"$PYTHON_BIN" "$TEMP_DIR/scrape_cargo_docs.py" "$LIBRARY_NAME" "$DOCS_DIR"

# Clean up temporary directory
print_status "Cleaning up temporary files..."
rm -rf "$TEMP_DIR"

# Flatten the directory structure
if [[ -d "$DOCS_DIR" ]]; then
    flatten_docs "$DOCS_DIR"
fi

# Show what we got
if [[ -d "$DOCS_DIR" ]]; then
    DOC_COUNT=$(find "$DOCS_DIR" -name "*.md" -type f | wc -l)
    if [[ $DOC_COUNT -gt 0 ]]; then
        print_success "Scraped $DOC_COUNT documentation pages for $LIBRARY_NAME"

        # Show directory structure
        print_status "Documentation structure:"
        # tree -d "$DOCS_DIR" 2>/dev/null || find "$DOCS_DIR" -type d | sort | head -20
        ls -la "$DOCS_DIR" | head -10

        # Show some example files
        print_status "Sample documentation files:"
        find "$DOCS_DIR" -name "*.md" -type f | head -10 | while read -r file; do
            # echo "  - $file"
            echo "  - $(basename "$file")"
        done

        if [[ $DOC_COUNT -gt 10 ]]; then
            echo "  ... and $((DOC_COUNT - 10)) more files"
        fi
    else
        print_error "No markdown files were created"
        exit 1
    fi
else
    print_error "$DOCS_DIR directory was not created properly"
    exit 1
fi

echo ""
print_success "$LIBRARY_NAME documentation scraping complete!"
echo ""
print_status "Documentation saved to: $DOCS_DIR/"
echo ""
print_status "To explore the documentation:"
echo "  find $DOCS_DIR -name \"*.md\" | head -20"
echo "  ls -la $DOCS_DIR/"
echo ""

# Show summary
print_status "Summary:"
echo "  Total pages scraped: $DOC_COUNT"
echo "  Library: $LIBRARY_NAME"
echo "  Output directory: $DOCS_DIR"
echo ""

print_success "Done! 🎉"