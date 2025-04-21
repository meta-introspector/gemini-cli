use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use reqwest::{Client, header::{HeaderMap, HeaderValue}};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};
use crate::config::MemoryBrokerConfig;

/// Common trait for all LLM clients
#[async_trait]
pub trait LLMClient: Send + Sync {
    /// Generate text from a prompt
    async fn generate(&self, prompt: &str) -> Result<String>;
    
    /// Get the provider name (for logging/debugging)
    fn provider_name(&self) -> &'static str;
    
    /// Get the model name being used
    fn model_name(&self) -> String;
}

//------------------------------------------------------------------------------
// Gemini Client
//------------------------------------------------------------------------------

/// Gemini API client (Google AI)
#[derive(Debug, Clone)]
pub struct GeminiClient {
    api_key: String,
    model_name: String,
    http_client: Client,
}

#[derive(Serialize)]
struct GeminiRequest<'a> {
    contents: Vec<GeminiContent<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    safety_settings: Option<Vec<GeminiSafetySetting>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_config: Option<GeminiGenerationConfig>,
}

#[derive(Serialize)]
struct GeminiContent<'a> {
    parts: Vec<GeminiPart<'a>>,
    role: &'static str,
}

#[derive(Serialize)]
struct GeminiPart<'a> {
    text: &'a str,
}

#[derive(Serialize)]
struct GeminiSafetySetting {
    category: String,
    threshold: String,
}

#[derive(Serialize)]
struct GeminiGenerationConfig {
    temperature: f32,
    max_output_tokens: u32,
    top_p: f32,
    top_k: u32,
}

#[derive(Deserialize, Debug)]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
    #[serde(default)]
    usage_metadata: Option<GeminiUsageMetadata>,
}

#[derive(Deserialize, Debug)]
struct GeminiCandidate {
    content: GeminiResponseContent,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize, Debug)]
struct GeminiResponseContent {
    parts: Vec<GeminiResponsePart>,
    role: String,
}

#[derive(Deserialize, Debug)]
struct GeminiResponsePart {
    text: String,
}

#[derive(Deserialize, Debug)]
struct GeminiUsageMetadata {
    prompt_token_count: u32,
    candidates_token_count: u32,
    total_token_count: u32,
}

#[derive(Deserialize, Debug)]
struct GeminiErrorResponse {
    error: GeminiError,
}

#[derive(Deserialize, Debug)]
struct GeminiError {
    code: u32,
    message: String,
    status: String,
}

impl GeminiClient {
    /// Create a new Gemini client
    pub fn new(api_key: &Option<String>, model_name: &Option<String>) -> Result<Self> {
        let api_key = api_key
            .as_ref()
            .ok_or_else(|| anyhow!("API key is required for Gemini"))?
            .clone();

        if api_key.is_empty() {
            return Err(anyhow!("Gemini API key cannot be empty"));
        }

        let model_name = model_name
            .as_ref()
            .cloned()
            .unwrap_or_else(|| "gemini-2.0-flash".to_string());

        // Build the HTTP client with reasonable timeouts
        let http_client = Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self {
            api_key,
            model_name,
            http_client,
        })
    }

    /// Build the Gemini API URL for the specified model
    fn api_url(&self) -> String {
        format!(
            "https://generativelanguage.googleapis.com/v1/models/{}:generateContent?key={}",
            self.model_name, self.api_key
        )
    }
}

#[async_trait]
impl LLMClient for GeminiClient {
    fn provider_name(&self) -> &'static str {
        "gemini"
    }

    fn model_name(&self) -> String {
        self.model_name.clone()
    }

    async fn generate(&self, prompt: &str) -> Result<String> {
        debug!("Generating text with Gemini model: {}", self.model_name);
        
        let request = GeminiRequest {
            contents: vec![GeminiContent {
                parts: vec![GeminiPart { text: prompt }],
                role: "user",
            }],
            safety_settings: None,
            generation_config: Some(GeminiGenerationConfig {
                temperature: 0.2,        // Use low temperature for more deterministic output
                max_output_tokens: 1024, // Reasonable size for broker's output
                top_p: 0.95,
                top_k: 40,
            }),
        };

        let response = self
            .http_client
            .post(&self.api_url())
            .json(&request)
            .send()
            .await
            .context("Failed to send request to Gemini API")?;

        let status = response.status();
        let response_text = response.text().await.context("Failed to read response")?;

        if !status.is_success() {
            // Try to parse as error response
            if let Ok(error_response) = serde_json::from_str::<GeminiErrorResponse>(&response_text) {
                return Err(anyhow!(
                    "Gemini API error: {} (code: {}, status: {})",
                    error_response.error.message,
                    error_response.error.code,
                    error_response.error.status
                ));
            } else {
                // Fallback error
                return Err(anyhow!(
                    "Gemini API request failed with status {}: {}",
                    status,
                    response_text
                ));
            }
        }

        // Parse successful response
        let gemini_response: GeminiResponse =
            serde_json::from_str(&response_text).context("Failed to parse Gemini response")?;

        // Extract the generated text
        if let Some(candidate) = gemini_response.candidates.first() {
            if let Some(part) = candidate.content.parts.first() {
                // Log token usage if available
                if let Some(usage) = &gemini_response.usage_metadata {
                    debug!(
                        "Gemini token usage: prompt={}, response={}, total={}",
                        usage.prompt_token_count,
                        usage.candidates_token_count,
                        usage.total_token_count
                    );
                }
                
                // Log finish reason if available
                if let Some(reason) = &candidate.finish_reason {
                    if reason != "STOP" {
                        warn!("Gemini generation finish reason: {}", reason);
                    }
                }
                
                return Ok(part.text.clone());
            }
        }

        Err(anyhow!("No text generated by Gemini"))
    }
}

//------------------------------------------------------------------------------
// Ollama Client
//------------------------------------------------------------------------------

/// Ollama API client for self-hosted LLMs
#[derive(Debug, Clone)]
pub struct OllamaClient {
    base_url: String,
    model_name: String,
    http_client: Client,
}

#[derive(Serialize)]
struct OllamaRequest<'a> {
    model: String,
    prompt: &'a str,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OllamaOptions>,
}

#[derive(Serialize)]
struct OllamaOptions {
    temperature: f32,
    top_p: f32,
    top_k: i32,
    num_predict: i32,
}

#[derive(Deserialize, Debug)]
struct OllamaResponse {
    model: String,
    created_at: String,
    response: String,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    total_duration: Option<u64>,
    #[serde(default)]
    load_duration: Option<u64>,
    #[serde(default)]
    prompt_eval_count: Option<u32>,
    #[serde(default)]
    eval_count: Option<u32>,
    #[serde(default)]
    eval_duration: Option<u64>,
}

impl OllamaClient {
    /// Create a new Ollama client
    pub fn new(base_url: &Option<String>, model_name: &Option<String>) -> Result<Self> {
        let base_url = base_url
            .as_ref()
            .ok_or_else(|| anyhow!("Base URL is required for Ollama"))?
            .clone();

        if base_url.is_empty() {
            return Err(anyhow!("Ollama base URL cannot be empty"));
        }

        let model_name = model_name
            .as_ref()
            .cloned()
            .unwrap_or_else(|| "llama2".to_string());

        // Build the HTTP client with reasonable timeouts
        let http_client = Client::builder()
            .timeout(Duration::from_secs(60)) // Ollama may be slower on local machines
            .connect_timeout(Duration::from_secs(10))
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            model_name,
            http_client,
        })
    }

    /// Build the Ollama API URL for generation
    fn api_url(&self) -> String {
        format!("{}/api/generate", self.base_url)
    }
}

#[async_trait]
impl LLMClient for OllamaClient {
    fn provider_name(&self) -> &'static str {
        "ollama"
    }

    fn model_name(&self) -> String {
        self.model_name.clone()
    }

    async fn generate(&self, prompt: &str) -> Result<String> {
        debug!("Generating text with Ollama model: {}", self.model_name);
        
        let request = OllamaRequest {
            model: self.model_name.clone(),
            prompt,
            stream: false, // We want a single response
            options: Some(OllamaOptions {
                temperature: 0.2, // Lower for more deterministic broker responses
                top_p: 0.9,
                top_k: 40,
                num_predict: 1024, // Reasonable size for broker output
            }),
        };

        let response = self
            .http_client
            .post(&self.api_url())
            .json(&request)
            .send()
            .await
            .context("Failed to send request to Ollama API")?;

        let status = response.status();
        let response_text = response.text().await.context("Failed to read Ollama response")?;

        if !status.is_success() {
            return Err(anyhow!(
                "Ollama API request failed with status {}: {}",
                status,
                response_text
            ));
        }

        // Parse successful response
        let ollama_response: OllamaResponse =
            serde_json::from_str(&response_text).context("Failed to parse Ollama response")?;

        // Log performance metrics if available
        if let (Some(total), Some(eval_count)) = (ollama_response.total_duration, ollama_response.eval_count) {
            debug!(
                "Ollama performance: total_duration={}ms, eval_tokens={}",
                total, eval_count
            );
        }

        if ollama_response.response.is_empty() {
            return Err(anyhow!("Ollama returned empty response"));
        }

        Ok(ollama_response.response)
    }
}

//------------------------------------------------------------------------------
// OpenAI Client
//------------------------------------------------------------------------------

/// OpenAI API client
#[derive(Debug, Clone)]
pub struct OpenAIClient {
    api_key: String,
    model_name: String,
    base_url: Option<String>, // For Azure or custom endpoints
    http_client: Client,
}

#[derive(Serialize)]
struct OpenAIRequest<'a> {
    model: String,
    messages: Vec<OpenAIMessage<'a>>,
    temperature: f32,
    max_tokens: u32,
    top_p: f32,
    frequency_penalty: f32,
    presence_penalty: f32,
}

#[derive(Serialize)]
struct OpenAIMessage<'a> {
    role: &'static str,
    content: &'a str,
}

#[derive(Deserialize, Debug)]
struct OpenAIResponse {
    id: String,
    object: String,
    created: u64,
    model: String,
    choices: Vec<OpenAIChoice>,
    usage: OpenAIUsage,
}

#[derive(Deserialize, Debug)]
struct OpenAIChoice {
    index: u32,
    message: OpenAIResponseMessage,
    finish_reason: String,
}

#[derive(Deserialize, Debug)]
struct OpenAIResponseMessage {
    role: String,
    content: String,
}

#[derive(Deserialize, Debug)]
struct OpenAIUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

#[derive(Deserialize, Debug)]
struct OpenAIErrorResponse {
    error: OpenAIError,
}

#[derive(Deserialize, Debug)]
struct OpenAIError {
    message: String,
    #[serde(rename = "type")]
    error_type: String,
    code: Option<String>,
}

impl OpenAIClient {
    /// Create a new OpenAI client
    pub fn new(api_key: &Option<String>, model_name: &Option<String>, base_url: &Option<String>) -> Result<Self> {
        let api_key = api_key
            .as_ref()
            .ok_or_else(|| anyhow!("API key is required for OpenAI"))?
            .clone();

        if api_key.is_empty() {
            return Err(anyhow!("OpenAI API key cannot be empty"));
        }

        let model_name = model_name
            .as_ref()
            .cloned()
            .unwrap_or_else(|| "gpt-3.5-turbo".to_string());
            
        let base_url = base_url.clone().filter(|url| !url.is_empty());

        // Build HTTP client with Authorization header
        let mut headers = HeaderMap::new();
        headers.insert(
            "Authorization",
            HeaderValue::from_str(&format!("Bearer {}", api_key))
                .context("Invalid API key format")?,
        );

        let http_client = Client::builder()
            .timeout(Duration::from_secs(45))
            .connect_timeout(Duration::from_secs(10))
            .default_headers(headers)
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self {
            api_key,
            model_name,
            base_url,
            http_client,
        })
    }

    /// Build the OpenAI API URL
    fn api_url(&self) -> String {
        self.base_url.clone().unwrap_or_else(|| "https://api.openai.com".to_string()) + "/v1/chat/completions"
    }
}

#[async_trait]
impl LLMClient for OpenAIClient {
    fn provider_name(&self) -> &'static str {
        "openai"
    }

    fn model_name(&self) -> String {
        self.model_name.clone()
    }

    async fn generate(&self, prompt: &str) -> Result<String> {
        debug!("Generating text with OpenAI model: {}", self.model_name);
        
        let request = OpenAIRequest {
            model: self.model_name.clone(),
            messages: vec![OpenAIMessage {
                role: "user",
                content: prompt,
            }],
            temperature: 0.2,
            max_tokens: 1024,
            top_p: 0.95,
            frequency_penalty: 0.0,
            presence_penalty: 0.0,
        };

        let response = self
            .http_client
            .post(&self.api_url())
            .json(&request)
            .send()
            .await
            .context("Failed to send request to OpenAI API")?;

        let status = response.status();
        let response_text = response.text().await.context("Failed to read OpenAI response")?;

        if !status.is_success() {
            // Try to parse as error response
            if let Ok(error_response) = serde_json::from_str::<OpenAIErrorResponse>(&response_text) {
                return Err(anyhow!(
                    "OpenAI API error: {} (type: {})",
                    error_response.error.message,
                    error_response.error.error_type
                ));
            } else {
                // Fallback error
                return Err(anyhow!(
                    "OpenAI API request failed with status {}: {}",
                    status,
                    response_text
                ));
            }
        }

        // Parse successful response
        let openai_response: OpenAIResponse =
            serde_json::from_str(&response_text).context("Failed to parse OpenAI response")?;

        // Log token usage
        debug!(
            "OpenAI token usage: prompt={}, completion={}, total={}",
            openai_response.usage.prompt_tokens,
            openai_response.usage.completion_tokens,
            openai_response.usage.total_tokens
        );

        // Extract the generated text
        if let Some(choice) = openai_response.choices.first() {
            return Ok(choice.message.content.clone());
        }

        Err(anyhow!("No text generated by OpenAI"))
    }
}

//------------------------------------------------------------------------------
// Anthropic Client (Claude)
//------------------------------------------------------------------------------

/// Anthropic API client
#[derive(Debug, Clone)]
pub struct AnthropicClient {
    api_key: String,
    model_name: String,
    http_client: Client,
}

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    prompt: String, // Note: Anthropic has a specific prompt format
    max_tokens_to_sample: u32,
    temperature: f32,
    top_p: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_k: Option<u32>,
}

#[derive(Deserialize, Debug)]
struct AnthropicResponse {
    completion: String,
    stop_reason: String,
    model: String,
}

#[derive(Deserialize, Debug)]
struct AnthropicErrorResponse {
    error: AnthropicError,
}

#[derive(Deserialize, Debug)]
struct AnthropicError {
    type_: String,
    message: String,
}

impl AnthropicClient {
    /// Create a new Anthropic client
    pub fn new(api_key: &Option<String>, model_name: &Option<String>) -> Result<Self> {
        let api_key = api_key
            .as_ref()
            .ok_or_else(|| anyhow!("API key is required for Anthropic"))?
            .clone();

        if api_key.is_empty() {
            return Err(anyhow!("Anthropic API key cannot be empty"));
        }

        let model_name = model_name
            .as_ref()
            .cloned()
            .unwrap_or_else(|| "claude-instant-1".to_string());

        // Build the HTTP client
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-api-key",
            HeaderValue::from_str(&api_key).context("Invalid API key format")?,
        );
        headers.insert(
            "anthropic-version",
            HeaderValue::from_static("2023-06-01"),
        );

        let http_client = Client::builder()
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(10))
            .default_headers(headers)
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self {
            api_key,
            model_name,
            http_client,
        })
    }

    /// Build the Anthropic API URL
    fn api_url(&self) -> &'static str {
        "https://api.anthropic.com/v1/complete"
    }
}

#[async_trait]
impl LLMClient for AnthropicClient {
    fn provider_name(&self) -> &'static str {
        "anthropic"
    }

    fn model_name(&self) -> String {
        self.model_name.clone()
    }

    async fn generate(&self, prompt: &str) -> Result<String> {
        debug!("Generating text with Anthropic model: {}", self.model_name);
        
        // Anthropic requires specific prompt format
        let formatted_prompt = format!("\n\nHuman: {}\n\nAssistant:", prompt);
        
        let request = AnthropicRequest {
            model: self.model_name.clone(),
            prompt: formatted_prompt,
            max_tokens_to_sample: 1024,
            temperature: 0.2,
            top_p: 0.95,
            top_k: Some(40),
        };

        let response = self
            .http_client
            .post(self.api_url())
            .json(&request)
            .send()
            .await
            .context("Failed to send request to Anthropic API")?;

        let status = response.status();
        let response_text = response.text().await.context("Failed to read Anthropic response")?;

        if !status.is_success() {
            // Try to parse as error response
            if let Ok(error_response) = serde_json::from_str::<AnthropicErrorResponse>(&response_text) {
                return Err(anyhow!(
                    "Anthropic API error: {} (type: {})",
                    error_response.error.message,
                    error_response.error.type_
                ));
            } else {
                // Fallback error
                return Err(anyhow!(
                    "Anthropic API request failed with status {}: {}",
                    status,
                    response_text
                ));
            }
        }

        // Parse successful response
        let anthropic_response: AnthropicResponse =
            serde_json::from_str(&response_text).context("Failed to parse Anthropic response")?;

        Ok(anthropic_response.completion.trim().to_string())
    }
}

//------------------------------------------------------------------------------
// Factory Function
//------------------------------------------------------------------------------

/// Creates an LLM client based on the provided configuration.
pub fn create_llm_client(config: &MemoryBrokerConfig) -> Result<Option<Arc<dyn LLMClient + Send + Sync>>> {
    match config.provider.as_deref() {
        Some("gemini") => {
            info!("Creating Gemini LLM client");
            let client = GeminiClient::new(&config.api_key, &config.model_name)
                .context("Failed to initialize Gemini client")?;
            Ok(Some(Arc::new(client)))
        }
        Some("ollama") => {
            info!("Creating Ollama LLM client");
            let client = OllamaClient::new(&config.base_url, &config.model_name)
                .context("Failed to initialize Ollama client")?;
            Ok(Some(Arc::new(client)))
        }
        Some("openai") => {
            info!("Creating OpenAI LLM client");
            let client = OpenAIClient::new(&config.api_key, &config.model_name, &config.base_url)
                .context("Failed to initialize OpenAI client")?;
            Ok(Some(Arc::new(client)))
        }
        Some("anthropic") => {
            info!("Creating Anthropic LLM client");
            let client = AnthropicClient::new(&config.api_key, &config.model_name)
                .context("Failed to initialize Anthropic client")?;
            Ok(Some(Arc::new(client)))
        }
        Some(other) => {
            warn!("Unsupported LLM provider '{}' specified in config", other);
            Ok(None)
        }
        None => {
            debug!("No LLM provider configured");
            Ok(None)
        }
    }
} 