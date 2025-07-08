// src/prompt.rs

//! Prompt Builder for Basic RAG
//!
//! This module takes the top‐K retrieved `Chunk`s and the user's question,
//! then formats them into a single prompt string suitable for sending to an LLM.
//!
//! Features:
//! - Token budget management to fit within context windows
//! - Chunk prioritization and truncation
//! - Flexible prompt templates for different LLM types
//! - Source attribution for traceability
//! - Graceful handling of edge cases (no chunks, oversized content)

use crate::ingest::Chunk;

/// Configuration for prompt building behavior
#[derive(Debug, Clone)]
pub struct PromptConfig {
    /// Maximum total tokens to use for the entire prompt
    pub max_context_tokens: usize,
    /// Maximum tokens to reserve for the question and system instructions
    pub reserved_tokens: usize,
    /// Maximum tokens per individual chunk (chunks will be truncated if longer)
    pub max_chunk_tokens: usize,
    /// Whether to include source attribution in the prompt
    pub include_sources: bool,
    /// Whether to include chunk position information
    pub include_positions: bool,
    /// Prompt template style
    pub template_style: PromptTemplateStyle,
}

impl Default for PromptConfig {
    fn default() -> Self {
        Self {
            max_context_tokens: 4000,  // Conservative default for GPT-3.5/4
            reserved_tokens: 500,      // Question + system instructions + answer prefix
            max_chunk_tokens: 300,     // Prevent any single chunk from dominating
            include_sources: true,
            include_positions: false,
            template_style: PromptTemplateStyle::ChatCompletion,
        }
    }
}

/// Different prompt template styles for various LLM APIs
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum PromptTemplateStyle {
    /// OpenAI Chat Completion format (system + user messages)
    ChatCompletion,
    /// Single completion prompt (instruction + context + question)
    Completion,
    /// Conversational format with explicit roles
    Conversational,
}

/// Build the LLM prompt from retrieved chunks and the user's question.
///
/// # Arguments
/// - `chunks`: The top‐K `Chunk`s from retrieval, in descending relevance order.
/// - `question`: The user's original question to answer.
///
/// # Returns
/// A single `String` ready to send as the prompt to the LLM.
pub fn build_prompt(chunks: &[Chunk], question: &str) -> String {
    build_prompt_with_config(chunks, question, &PromptConfig::default())
}

/// Build the LLM prompt with custom configuration.
///
/// # Arguments
/// - `chunks`: The top‐K `Chunk`s from retrieval, in descending relevance order.
/// - `question`: The user's original question to answer.
/// - `config`: Configuration for prompt building behavior.
///
/// # Returns
/// A single `String` ready to send as the prompt to the LLM.
pub fn build_prompt_with_config(chunks: &[Chunk], question: &str, config: &PromptConfig) -> String {
    // Calculate available tokens for chunk content
    let question_tokens = estimate_tokens(question);
    let system_tokens = estimate_tokens(get_system_instructions());
    let used_reserved = question_tokens + system_tokens + 100; // 100 for formatting overhead
    
    let available_tokens = config.max_context_tokens.saturating_sub(
        config.reserved_tokens.max(used_reserved)
    );

    // Select and prepare chunks that fit within token budget
    let prepared_chunks = prepare_chunks(chunks, available_tokens, config);
    
    // Build the prompt based on template style
    match config.template_style {
        PromptTemplateStyle::ChatCompletion => build_chat_completion_prompt(&prepared_chunks, question, config),
        PromptTemplateStyle::Completion => build_completion_prompt(&prepared_chunks, question, config),
        PromptTemplateStyle::Conversational => build_conversational_prompt(&prepared_chunks, question, config),
    }
}

/// Prepare chunks for inclusion in the prompt, handling token limits and truncation.
fn prepare_chunks(chunks: &[Chunk], available_tokens: usize, config: &PromptConfig) -> Vec<PreparedChunk> {
    let mut prepared = Vec::new();
    let mut used_tokens = 0;
    
    for (index, chunk) in chunks.iter().enumerate() {
        // Estimate tokens for this chunk including formatting
        let chunk_text = truncate_chunk_text(&chunk.text, config.max_chunk_tokens);
        let formatted_chunk = format_chunk_for_estimation(chunk, &chunk_text, index, config);
        let chunk_tokens = estimate_tokens(&formatted_chunk);
        
        // Check if we can fit this chunk
        if used_tokens + chunk_tokens > available_tokens {
            // If this is the first chunk and it's too big, include a truncated version
            if prepared.is_empty() {
                let truncated_text = truncate_to_token_limit(&chunk.text, available_tokens / 2);
                prepared.push(PreparedChunk {
                    text: truncated_text,
                    source: chunk.source.clone(),
                    position: chunk.position,
                    index,
                    truncated: true,
                });
            }
            break;
        }
        
        prepared.push(PreparedChunk {
            text: chunk_text.clone(),
            source: chunk.source.clone(),
            position: chunk.position,
            index,
            truncated: chunk_text.len() < chunk.text.len(), 
        });
        
        used_tokens += chunk_tokens;
    }
    
    prepared
}

/// Internal representation of a chunk prepared for prompt inclusion
#[derive(Debug, Clone)]
struct PreparedChunk {
    text: String,
    source: String,
    position: usize,
    index: usize,
    truncated: bool,
}

/// Build a Chat Completion style prompt (system + user messages)
fn build_chat_completion_prompt(chunks: &[PreparedChunk], question: &str, config: &PromptConfig) -> String {
    let mut prompt = String::new();
    
    // System message
    prompt.push_str("SYSTEM: ");
    prompt.push_str(get_system_instructions());
    prompt.push_str("\n\n");
    
    // User message with context and question
    prompt.push_str("USER: ");
    
    if chunks.is_empty() {
        prompt.push_str("No relevant documentation was found for this question. Please answer based on your general knowledge, but indicate that you don't have specific documentation context.\n\n");
    } else {
        prompt.push_str("Here are relevant documentation excerpts to help answer the question:\n\n");
        
        for chunk in chunks {
            format_chunk_into_prompt(chunk, &mut prompt, config);
        }
        
        prompt.push_str("\n");
    }
    
    prompt.push_str(&format!("Question: {}\n\n", question));
    prompt.push_str("Please provide a comprehensive answer based on the documentation excerpts above. If the excerpts don't contain enough information to fully answer the question, please indicate what information is missing.");
    
    prompt
}

/// Build a Completion style prompt (single text block)
fn build_completion_prompt(chunks: &[PreparedChunk], question: &str, config: &PromptConfig) -> String {
    let mut prompt = String::new();
    
    // Instructions
    prompt.push_str(get_system_instructions());
    prompt.push_str("\n\n");
    
    // Context
    if chunks.is_empty() {
        prompt.push_str("No documentation excerpts found.\n\n");
    } else {
        prompt.push_str("Documentation excerpts:\n\n");
        
        for chunk in chunks {
            format_chunk_into_prompt(chunk, &mut prompt, config);
        }
        
        prompt.push_str("\n");
    }
    
    // Question and answer prompt
    prompt.push_str(&format!("Question: {}\n\n", question));
    prompt.push_str("Answer: ");
    
    prompt
}

/// Build a Conversational style prompt
fn build_conversational_prompt(chunks: &[PreparedChunk], question: &str, config: &PromptConfig) -> String {
    let mut prompt = String::new();
    
    prompt.push_str("Human: I have a question about some documentation. Let me provide you with relevant excerpts first.\n\n");
    
    if chunks.is_empty() {
        prompt.push_str("Actually, I couldn't find any relevant documentation excerpts for this question.\n\n");
    } else {
        for chunk in chunks {
            format_chunk_into_prompt(chunk, &mut prompt, config);
        }
        prompt.push_str("\n");
    }
    
    prompt.push_str(&format!("My question is: {}\n\n", question));
    prompt.push_str("Assistant: ");
    
    prompt
}

/// Format a single chunk into the prompt string
fn format_chunk_into_prompt(chunk: &PreparedChunk, prompt: &mut String, config: &PromptConfig) {
    // Start with excerpt number
    prompt.push_str(&format!("[{}] ", chunk.index + 1));
    
    // Add source information if enabled
    if config.include_sources {
        prompt.push_str(&format!("(source: {}", chunk.source));
        
        // Add position if enabled
        if config.include_positions {
            prompt.push_str(&format!(", chunk: {}", chunk.position));
        }
        
        // Add truncation indicator if needed
        if chunk.truncated {
            prompt.push_str(", truncated");
        }
        
        prompt.push_str(")\n");
    }
    
    // Add the chunk text
    prompt.push_str(&chunk.text);
    prompt.push_str("\n\n");
}

/// Helper function for token estimation during chunk preparation
fn format_chunk_for_estimation(chunk: &Chunk, text: &str, index: usize, config: &PromptConfig) -> String {
    let mut formatted = format!("[{}] ", index + 1);
    
    if config.include_sources {
        formatted.push_str(&format!("(source: {}", chunk.source));
        
        if config.include_positions {
            formatted.push_str(&format!(", chunk: {}", chunk.position));
        }
        
        formatted.push_str(")\n");
    }
    
    formatted.push_str(text);
    formatted.push_str("\n\n");
    
    formatted
}

/// Get system instructions for the LLM
fn get_system_instructions() -> &'static str {
    "You are a helpful assistant that answers questions based on provided documentation excerpts. \
     When answering:
     - Base your response primarily on the provided excerpts
     - If the excerpts don't contain enough information, clearly indicate what's missing
     - Include relevant details and examples from the documentation
     - Maintain accuracy and don't hallucinate information not present in the excerpts
     - If no excerpts are provided, indicate that you're answering from general knowledge
     Always finish with a working example/use case of what the user asked."
}

/// Estimate the number of tokens in a text string.
/// This is a rough approximation: 1 token ≈ 4 characters for English text.
/// For more accurate counting, you'd want to use the actual tokenizer for your model.
fn estimate_tokens(text: &str) -> usize {
    // Simple heuristic: average of 4 characters per token
    // This is conservative for most models (GPT-3.5/4, Claude, etc.)
    (text.len() as f64 / 4.0).ceil() as usize
}

/// Safe truncate at char boundary, fallback if too short.
fn safe_truncate(text: &str, max_chars: usize) -> &str {
    if text.is_char_boundary(max_chars) {
        &text[..max_chars]
    } else {
        // Walk backwards until we find a valid char boundary
        let mut idx = max_chars;
        while idx > 0 && !text.is_char_boundary(idx) {
            idx -= 1;
        }
        &text[..idx]
    }
}

/// Truncate chunk text to fit within a token limit while preserving readability.
fn truncate_chunk_text(text: &str, max_tokens: usize) -> String {
    let max_chars = max_tokens * 4; // Conservative estimate
    
    if text.len() <= max_chars {
        return text.to_string();
    }
    
    // Try to truncate at sentence boundaries
    let truncated = safe_truncate(text, max_chars);    
    
    // Find the last sentence boundary (period, exclamation, or question mark followed by space)
    if let Some(last_sentence) = truncated.rfind(|c: char| c == '.' || c == '!' || c == '?') {
        let candidate = &truncated[..=last_sentence];
        // Make sure we're not truncating too aggressively (at least 50% of target length)
        if candidate.len() >= max_chars / 2 {
            return candidate.to_string();
        }
    }
    
    // If no good sentence boundary, truncate at word boundary
    if let Some(last_space) = truncated.rfind(' ') {
        let candidate = &truncated[..last_space];
        if candidate.len() >= max_chars / 2 {
            return format!("{}...", candidate);
        }
    }
    
    // Fallback: hard truncate with ellipsis
    format!("{}...", &text[..max_chars.saturating_sub(3)])
}

/// Truncate text to fit within a specific token limit (more aggressive than chunk truncation)
fn truncate_to_token_limit(text: &str, max_tokens: usize) -> String {
    let max_chars = max_tokens * 4;
    
    if text.len() <= max_chars {
        return text.to_string();
    }
    
    // More aggressive truncation for token limits
    let truncated = &text[..max_chars.saturating_sub(10)];
    
    // Try to end at a reasonable boundary
    if let Some(last_period) = truncated.rfind(". ") {
        return format!("{}.", &truncated[..last_period]);
    }
    
    if let Some(last_space) = truncated.rfind(' ') {
        return format!("{}...", &truncated[..last_space]);
    }
    
    format!("{}...", truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_empty_chunks() {
        let chunks = vec![];
        let question = "What is the meaning of life?";
        let prompt = build_prompt(&chunks, question);
        
        assert!(prompt.contains("No relevant documentation"));
        assert!(prompt.contains(question));
    }

    #[test]
    fn test_single_chunk() {
        let chunks = vec![create_test_chunk(
            "test:1",
            "This is a test chunk with some information about testing.",
            "test.md",
            0
        )];
        let question = "How do I test?";
        let prompt = build_prompt(&chunks, question);
        
        assert!(prompt.contains("[1]"));
        assert!(prompt.contains("test.md"));
        assert!(prompt.contains("test chunk"));
        assert!(prompt.contains(question));
    }

    #[test]
    fn test_multiple_chunks() {
        let chunks = vec![
            create_test_chunk("test:1", "First chunk", "test1.md", 0),
            create_test_chunk("test:2", "Second chunk", "test2.md", 1),
            create_test_chunk("test:3", "Third chunk", "test3.md", 2),
        ];
        let question = "Test question";
        let prompt = build_prompt(&chunks, question);
        
        assert!(prompt.contains("[1]"));
        assert!(prompt.contains("[2]"));
        assert!(prompt.contains("[3]"));
        assert!(prompt.contains("First chunk"));
        assert!(prompt.contains("Second chunk"));
        assert!(prompt.contains("Third chunk"));
    }

    #[test]
    fn test_token_estimation() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("test"), 1);
        assert_eq!(estimate_tokens("this is a test"), 4);
        assert_eq!(estimate_tokens(&"a".repeat(100)), 25);
    }

    #[test]
    fn test_chunk_truncation() {
        let long_text = "This is a very long text. ".repeat(50);
        let truncated = truncate_chunk_text(&long_text, 10); // 10 tokens = ~40 chars
        
        assert!(truncated.len() < long_text.len());
        assert!(truncated.len() <= 40);
    }

    #[test]
    fn test_different_template_styles() {
        let chunks = vec![create_test_chunk("test:1", "Test content", "test.md", 0)];
        let question = "Test question";
        
        let config_chat = PromptConfig {
            template_style: PromptTemplateStyle::ChatCompletion,
            ..Default::default()
        };
        let config_completion = PromptConfig {
            template_style: PromptTemplateStyle::Completion,
            ..Default::default()
        };
        let config_conversational = PromptConfig {
            template_style: PromptTemplateStyle::Conversational,
            ..Default::default()
        };
        
        let prompt_chat = build_prompt_with_config(&chunks, question, &config_chat);
        let prompt_completion = build_prompt_with_config(&chunks, question, &config_completion);
        let prompt_conversational = build_prompt_with_config(&chunks, question, &config_conversational);
        
        assert!(prompt_chat.contains("SYSTEM:"));
        assert!(prompt_chat.contains("USER:"));
        
        assert!(prompt_completion.contains("Answer: "));
        assert!(!prompt_completion.contains("SYSTEM:"));
        
        assert!(prompt_conversational.contains("Human:"));
        assert!(prompt_conversational.contains("Assistant:"));
    }

    #[test]
    fn test_source_attribution_toggle() {
        let chunks = vec![create_test_chunk("test:1", "Test content", "test.md", 0)];
        let question = "Test question";
        
        let config_with_sources = PromptConfig {
            include_sources: true,
            ..Default::default()
        };
        let config_without_sources = PromptConfig {
            include_sources: false,
            ..Default::default()
        };
        
        let prompt_with = build_prompt_with_config(&chunks, question, &config_with_sources);
        let prompt_without = build_prompt_with_config(&chunks, question, &config_without_sources);
        
        assert!(prompt_with.contains("test.md"));
        assert!(!prompt_without.contains("test.md"));
    }
}