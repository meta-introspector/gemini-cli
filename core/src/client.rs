use reqwest::Client;

use crate::config::GeminiConfig;
use crate::errors::{GeminiError, GeminiResult};
use crate::types::*;

/// Client for interacting with the Gemini API
#[derive(Debug, Clone)]
pub struct GeminiClient {
    client: Client,
    config: GeminiConfig,
    model: GeminiModel,
}

impl GeminiClient {
    /// Create a new Gemini API client
    pub fn new(config: GeminiConfig) -> GeminiResult<Self> {
        let api_key = config.api_key.clone().ok_or_else(|| {
            GeminiError::ConfigError(
                "API key is required to initialize the Gemini client".to_string(),
            )
        })?;

        let model = GeminiModel::new(api_key, config.model_name.clone());

        let client = Client::new();

        Ok(Self {
            client,
            config,
            model,
        })
    }

    /// Get the base API URL
    fn get_base_url(&self) -> String {
        format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model.model_name, self.model.api_key
        )
    }

    /// Generate content using the Gemini API
    pub async fn generate_content(
        &self,
        request: GenerateContentRequest,
    ) -> GeminiResult<GenerateContentResponse> {
        let url = self.get_base_url();

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| GeminiError::RequestError(format!("Failed to send request: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            let error_body = response.text().await.map_err(|e| {
                GeminiError::ResponseError(format!("Failed to read error response: {}", e))
            })?;

            return Err(GeminiError::HttpError {
                status_code: status.as_u16(),
                message: format!("API request failed: {}", error_body),
            });
        }

        let response_body = response
            .json::<GenerateContentResponse>()
            .await
            .map_err(|e| GeminiError::ParsingError(format!("Failed to parse response: {}", e)))?;

        Ok(response_body)
    }

    /// Creates a GenerateContentRequest with the given user message.
    ///
    /// This is a convenience method for simple single-turn chat interactions.
    pub(crate) fn create_chat_request(&self, user_message: &str) -> GenerateContentRequest {
        let system_instruction = self.config.system_prompt.as_ref().map(|prompt| Content {
            parts: vec![Part::text(prompt.clone())],
            role: Some("system".to_string()),
        });

        let user_content = Content {
            parts: vec![Part::text(user_message.to_string())],
            role: Some("user".to_string()),
        };

        GenerateContentRequest {
            contents: vec![user_content],
            system_instruction,
            tools: None,
            generation_config: Some(GenerationConfig {
                temperature: Some(0.7),
                top_p: None,
                top_k: None,
                candidate_count: None,
                max_output_tokens: None,
                response_mime_type: None,
            }),
        }
    }

    /// Helper method to extract text from a response
    pub fn extract_text_from_response(
        &self,
        response: &GenerateContentResponse,
    ) -> GeminiResult<String> {
        if response.candidates.is_empty() {
            return Err(GeminiError::ResponseError(
                "No candidates in response".to_string(),
            ));
        }

        let candidate = &response.candidates[0];
        let content = candidate
            .content
            .as_ref()
            .ok_or_else(|| GeminiError::ResponseError("No content in candidate".to_string()))?;

        if content.parts.is_empty() {
            return Err(GeminiError::ResponseError(
                "No parts in content".to_string(),
            ));
        }

        let part = &content.parts[0];
        let text = part
            .text
            .as_ref()
            .ok_or_else(|| GeminiError::ResponseError("No text in part".to_string()))?;

        Ok(text.clone())
    }

    /// Helper method to extract function calls from a response
    pub fn extract_function_calls_from_response(
        &self,
        response: &GenerateContentResponse,
    ) -> Vec<FunctionCall> {
        let mut function_calls = Vec::new();

        if let Some(candidate) = response.candidates.first() {
            if let Some(content) = &candidate.content {
                for part in &content.parts {
                    if let Some(function_call) = &part.function_call {
                        function_calls.push(function_call.clone());
                    }
                }
            }
        }

        function_calls
    }

    /// Simple chat method that handles creating the request and extracting the response
    pub async fn chat(&self, message: &str) -> GeminiResult<String> {
        let request = self.create_chat_request(message);
        let response = self.generate_content(request).await?;
        self.extract_text_from_response(&response)
    }
}

/// A simple chat message representation used for building chat history
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn user(content: String) -> Self {
        Self {
            role: "user".to_string(),
            content,
        }
    }

    pub fn assistant(content: String) -> Self {
        Self {
            role: "assistant".to_string(),
            content,
        }
    }

    pub fn system(content: String) -> Self {
        Self {
            role: "system".to_string(),
            content,
        }
    }
}
