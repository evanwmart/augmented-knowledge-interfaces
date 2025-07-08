// src/llm.rs

//! LLM integration module for Basic RAG
//!
//! This module handles sending assembled prompts to OpenAI's Chat API and returning
//! the generated answer as a String. It includes proper error handling, request
//! configuration, and response parsing.

use anyhow::{anyhow, Context, Result};
use log::{debug, info, warn, error};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Configuration for OpenAI API requests
const OPENAI_API_URL: &str = "https://api.openai.com/v1/chat/completions";
const DEFAULT_MODEL: &str = "gpt-4o";
const DEFAULT_MAX_TOKENS: u32 = 2048;
const DEFAULT_TEMPERATURE: f32 = 0.1;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// OpenAI Chat API request structure
#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<Message>,
    max_tokens: u32,
    temperature: f32,
    stream: bool,
}

/// Message structure for OpenAI Chat API
#[derive(Debug, Serialize)]
struct Message {
    role: String,
    content: String,
}

/// OpenAI Chat API response structure
#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<Choice>,
    #[serde(default)]
    error: Option<ApiError>,
}

/// Choice structure from OpenAI response
#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
    finish_reason: Option<String>,
}

/// Response message structure
#[derive(Debug, Deserialize)]
struct ResponseMessage {
    content: String,
}

/// API error structure
#[derive(Debug, Deserialize)]
struct ApiError {
    message: String,
    #[serde(rename = "type")]
    error_type: String,
    _code: Option<String>,
}

/// Send the given prompt to the OpenAI Chat API and return the completion text.
///
/// # Arguments
/// * `api_key` - OpenAI API key for authentication
/// * `prompt` - The assembled prompt string to send to the LLM
///
/// # Returns
/// * `Ok(String)` - The generated answer from the LLM
/// * `Err(anyhow::Error)` - Any error that occurred during the request
///
/// # Examples
/// ```rust,no_run
/// # use anyhow::Result;
/// # async fn example() -> Result<()> {
/// let api_key = "your-api-key";
/// let prompt = "What is Rust?";
/// let answer = query_llm(api_key, prompt).await?;
/// println!("Answer: {}", answer);
/// # Ok(())
/// # }
/// ```
pub async fn query_llm(api_key: &str, prompt: &str) -> Result<String> {
    info!("query_llm - Starting LLM query process");
    debug!("query_llm - Received prompt with length: {} chars", prompt.len());
    
    // Validate inputs
    if api_key.is_empty() {
        error!("query_llm - API key validation failed: empty key provided");
        return Err(anyhow!("OpenAI API key is required but not provided"));
    }
    debug!("query_llm - API key validation passed");
    
    if prompt.is_empty() {
        error!("query_llm - Prompt validation failed: empty prompt provided");
        return Err(anyhow!("Prompt cannot be empty"));
    }
    debug!("query_llm - Prompt validation passed");

    info!("query_llm - Sending prompt to OpenAI (length: {} chars)", prompt.len());
    debug!("query_llm - Prompt content: {}", prompt);

    // Build the request
    debug!("query_llm - Building chat completion request");
    let request = build_chat_request(prompt);
    debug!("query_llm - Request built successfully: {:?}", request);

    // Create HTTP client
    debug!("query_llm - Creating HTTP client with timeout: {:?}", REQUEST_TIMEOUT);
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .context("Failed to create HTTP client")?;
    debug!("query_llm - HTTP client created successfully");

    // Send the request
    info!("query_llm - Sending HTTP request to OpenAI API");
    debug!("query_llm - Request URL: {}", OPENAI_API_URL);
    let response = client
        .post(OPENAI_API_URL)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request)
        .send()
        .await
        .context("Failed to send request to OpenAI API")?;

    debug!("query_llm - Received HTTP response with status: {}", response.status());

    // Check response status
    if !response.status().is_success() {
        let status = response.status();
        warn!("query_llm - HTTP request failed with status: {}", status);
        let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        error!("query_llm - API error response: {}", error_text);
        return Err(anyhow!(
            "OpenAI API request failed with status {}: {}",
            status,
            error_text
        ));
    }

    // Parse response
    debug!("query_llm - Reading response body");
    let response_text = response.text().await
        .context("Failed to read response body")?;
    
    debug!("query_llm - Response body length: {} chars", response_text.len());
    debug!("query_llm - Raw response: {}", response_text);

    debug!("query_llm - Parsing JSON response");
    let chat_response: ChatCompletionResponse = serde_json::from_str(&response_text)
        .context("Failed to parse OpenAI API response")?;
    debug!("query_llm - JSON parsing successful");

    // Handle API errors
    if let Some(error) = chat_response.error {
        error!("query_llm - OpenAI API returned error: {} ({})", error.message, error.error_type);
        return Err(anyhow!(
            "OpenAI API error ({}): {}",
            error.error_type,
            error.message
        ));
    }

    // Extract the answer
    debug!("query_llm - Extracting answer from response");
    let answer = extract_answer_from_response(chat_response)?;
    
    info!("query_llm - Successfully received answer from OpenAI (length: {} chars)", answer.len());
    debug!("query_llm - Answer: {}", answer);

    Ok(answer)
}

/// Build a chat completion request from the prompt
fn build_chat_request(prompt: &str) -> ChatCompletionRequest {
    debug!("build_chat_request - Creating system message");
    let system_message = Message {
        role: "system".to_string(),
        content: "You are a helpful assistant that answers questions based on provided documentation. Be concise and accurate. If you cannot answer based on the provided context, say so clearly.".to_string(),
    };

    debug!("build_chat_request - Creating user message with prompt length: {}", prompt.len());
    let user_message = Message {
        role: "user".to_string(),
        content: prompt.to_string(),
    };

    debug!("build_chat_request - Building request with model: {}, max_tokens: {}, temperature: {}", 
           DEFAULT_MODEL, DEFAULT_MAX_TOKENS, DEFAULT_TEMPERATURE);
    
    let request = ChatCompletionRequest {
        model: DEFAULT_MODEL.to_string(),
        messages: vec![system_message, user_message],
        max_tokens: DEFAULT_MAX_TOKENS,
        temperature: DEFAULT_TEMPERATURE,
        stream: false,
    };

    debug!("build_chat_request - Request created successfully with {} messages", request.messages.len());
    request
}

/// Extract the answer text from the OpenAI response
fn extract_answer_from_response(response: ChatCompletionResponse) -> Result<String> {
    debug!("extract_answer_from_response - Processing response with {} choices", response.choices.len());
    
    if response.choices.is_empty() {
        error!("extract_answer_from_response - No choices returned from OpenAI API");
        return Err(anyhow!("No choices returned from OpenAI API"));
    }

    let choice = &response.choices[0];
    debug!("extract_answer_from_response - Using first choice");
    
    // Check finish reason
    if let Some(finish_reason) = &choice.finish_reason {
        debug!("extract_answer_from_response - Finish reason: {}", finish_reason);
        match finish_reason.as_str() {
            "stop" => {
                debug!("extract_answer_from_response - Normal completion");
            }
            "length" => {
                warn!("extract_answer_from_response - Response was truncated due to max_tokens limit");
            }
            "content_filter" => {
                error!("extract_answer_from_response - Response was filtered due to content policy");
                return Err(anyhow!("Response was filtered due to content policy"));
            }
            other => {
                warn!("extract_answer_from_response - Unexpected finish reason: {}", other);
            }
        }
    } else {
        debug!("extract_answer_from_response - No finish reason provided");
    }

    let content = choice.message.content.trim();
    debug!("extract_answer_from_response - Content length after trimming: {}", content.len());
    
    if content.is_empty() {
        error!("extract_answer_from_response - Empty response content from OpenAI API");
        return Err(anyhow!("Empty response from OpenAI API"));
    }

    debug!("extract_answer_from_response - Successfully extracted answer");
    Ok(content.to_string())
}

/// Alternative implementation for local LLM support (placeholder)
/// This would be used if the user wants to use a local model instead of OpenAI
#[allow(dead_code)]
async fn query_local_llm(_model_path: &str, _prompt: &str) -> Result<String> {
    warn!("query_local_llm - Local LLM support not yet implemented");
    debug!("query_local_llm - Model path: {}", _model_path);
    debug!("query_local_llm - Prompt length: {}", _prompt.len());
    
    // TODO: Implement local LLM support using the `llm` crate
    // This would involve:
    // 1. Loading a GGUF model from disk
    // 2. Tokenizing the prompt
    // 3. Running inference with appropriate parameters
    // 4. Decoding the output tokens back to text
    
    Err(anyhow!("Local LLM support not yet implemented"))
}

/// Estimate the number of tokens in a text string
/// This is a rough approximation used for token budget management
#[allow(dead_code)]
fn estimate_tokens(text: &str) -> usize {
    debug!("estimate_tokens - Estimating tokens for text length: {}", text.len());
    
    // Rough approximation: 1 token ≈ 4 characters for English text
    // This is not exact but good enough for budget estimation
    let estimated = (text.len() as f64 / 4.0).ceil() as usize;
    
    debug!("estimate_tokens - Estimated {} tokens", estimated);
    estimated
}

/// Configuration struct for customizing LLM behavior
#[allow(dead_code)]
pub struct LlmConfig {
    pub model: String,
    pub max_tokens: u32,
    pub temperature: f32,
    pub timeout: Duration,
}

impl Default for LlmConfig {
    fn default() -> Self {
        debug!("LlmConfig::default - Creating default configuration");
        Self {
            model: DEFAULT_MODEL.to_string(),
            max_tokens: DEFAULT_MAX_TOKENS,
            temperature: DEFAULT_TEMPERATURE,
            timeout: REQUEST_TIMEOUT,
        }
    }
}

/// Enhanced version of query_llm with custom configuration
#[allow(dead_code)]
pub async fn query_llm_with_config(
    api_key: &str,
    prompt: &str,
    config: &LlmConfig,
) -> Result<String> {
    info!("query_llm_with_config - Starting LLM query with custom config");
    debug!("query_llm_with_config - Config: model={}, max_tokens={}, temperature={}, timeout={:?}", 
           config.model, config.max_tokens, config.temperature, config.timeout);
    
    // This would be similar to query_llm but with customizable parameters
    // For now, just delegate to the main function
    warn!("query_llm_with_config - Custom config not yet implemented, delegating to default query_llm");
    query_llm(api_key, prompt).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens() {
        debug!("test_estimate_tokens - Running token estimation tests");
        assert_eq!(estimate_tokens("hello world"), 3);
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("a"), 1);
        debug!("test_estimate_tokens - All tests passed");
    }

    #[test]
    fn test_build_chat_request() {
        debug!("test_build_chat_request - Testing chat request building");
        let prompt = "What is Rust?";
        let request = build_chat_request(prompt);
        
        assert_eq!(request.model, DEFAULT_MODEL);
        assert_eq!(request.messages.len(), 2);
        assert_eq!(request.messages[0].role, "system");
        assert_eq!(request.messages[1].role, "user");
        assert_eq!(request.messages[1].content, prompt);
        assert!(!request.stream);
        debug!("test_build_chat_request - All assertions passed");
    }

    #[test]
    fn test_extract_answer_success() {
        debug!("test_extract_answer_success - Testing successful answer extraction");
        let response = ChatCompletionResponse {
            choices: vec![Choice {
                message: ResponseMessage {
                    content: "Rust is a systems programming language.".to_string(),
                },
                finish_reason: Some("stop".to_string()),
            }],
            error: None,
        };

        let answer = extract_answer_from_response(response).unwrap();
        assert_eq!(answer, "Rust is a systems programming language.");
        debug!("test_extract_answer_success - Test passed");
    }

    #[test]
    fn test_extract_answer_empty_choices() {
        debug!("test_extract_answer_empty_choices - Testing empty choices handling");
        let response = ChatCompletionResponse {
            choices: vec![],
            error: None,
        };

        let result = extract_answer_from_response(response);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No choices returned"));
        debug!("test_extract_answer_empty_choices - Test passed");
    }

    #[test]
    fn test_extract_answer_empty_content() {
        debug!("test_extract_answer_empty_content - Testing empty content handling");
        let response = ChatCompletionResponse {
            choices: vec![Choice {
                message: ResponseMessage {
                    content: "".to_string(),
                },
                finish_reason: Some("stop".to_string()),
            }],
            error: None,
        };

        let result = extract_answer_from_response(response);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Empty response"));
        debug!("test_extract_answer_empty_content - Test passed");
    }

    #[tokio::test]
    async fn test_query_llm_empty_api_key() {
        debug!("test_query_llm_empty_api_key - Testing empty API key handling");
        let result = query_llm("", "test prompt").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("API key is required"));
        debug!("test_query_llm_empty_api_key - Test passed");
    }

    #[tokio::test]
    async fn test_query_llm_empty_prompt() {
        debug!("test_query_llm_empty_prompt - Testing empty prompt handling");
        let result = query_llm("test-key", "").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Prompt cannot be empty"));
        debug!("test_query_llm_empty_prompt - Test passed");
    }
}