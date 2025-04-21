use anyhow::{anyhow, Result};
use gemini_core::client::GeminiClient;
use gemini_core::types::{Content, GenerateContentRequest, Part, Tool};
use gemini_mcp::gemini::{FunctionCall, parse_function_calls};
use serde_json::Value;
use thiserror::Error;
use tracing::{debug, error};

#[derive(Error, Debug)]
pub enum LlmClientError {
    #[error("API call failed: {0}")]
    ApiError(String),
    
    #[error("Failed to parse response: {0}")]
    ParseError(String),
}

/// Generate a response from the LLM using the provided client
/// 
/// Returns a tuple of (response_text, function_calls)
pub async fn generate_response(
    client: &GeminiClient,
    prompt: &str,
    system_prompt: &str,
    tools: Option<&[Tool]>,
) -> Result<(String, Vec<FunctionCall>)> {
    debug!(
        prompt = prompt,
        system_prompt = system_prompt,
        has_tools = tools.is_some(),
        "Sending prompt to LLM"
    );

    // Create the system instruction
    let system_instruction = Some(Content {
        parts: vec![Part::text(system_prompt.to_string())],
        role: Some("system".to_string()),
    });

    // Create the user content
    let user_content = Content {
        parts: vec![Part::text(prompt.to_string())],
        role: Some("user".to_string()),
    };

    let request = GenerateContentRequest {
        contents: vec![user_content],
        system_instruction,
        tools: tools.map(|t| t.to_vec()),
        generation_config: None,
    };

    match client.generate_content(request).await {
        Ok(response) => {
            debug!("Received response from LLM");
            
            // Extract text
            let text = client.extract_text_from_response(&response)
                .map_err(|e| anyhow!("Failed to extract text from response: {}", e))?;
            
            // Extract function calls
            let function_calls: Vec<FunctionCall> = client.extract_function_calls_from_response(&response)
                .into_iter()
                .map(|fc| FunctionCall {
                    name: fc.name,
                    arguments: fc.arguments,
                })
                .collect();
            
            debug!(
                function_calls_count = function_calls.len(),
                "Extracted function calls from response"
            );
            
            Ok((text, function_calls))
        }
        Err(e) => {
            error!(error = %e, "API call to LLM failed");
            Err(anyhow!("API call to LLM failed: {}", e))
        }
    }
}

/// Extract text from a Gemini API response
pub fn extract_text_from_response(client: &GeminiClient, response: &Value) -> Option<String> {
    // Convert to a proper response structure first
    if let Ok(response_str) = serde_json::to_string(response) {
        if let Ok(typed_response) = serde_json::from_str::<gemini_core::types::GenerateContentResponse>(&response_str) {
            return client.extract_text_from_response(&typed_response).ok();
        }
    }
    None
}

/// Extract function calls from a Gemini API response
pub fn parse_function_calls_from_json(response: &Value) -> Vec<FunctionCall> {
    if let Ok(response_str) = serde_json::to_string(response) {
        if let Ok(typed_response) = serde_json::from_str::<gemini_core::types::GenerateContentResponse>(&response_str) {
            return typed_response.candidates.iter()
                .filter_map(|c| c.content.as_ref())
                .flat_map(|content| &content.parts)
                .filter_map(|part| part.function_call.clone())
                .map(|fc| FunctionCall {
                    name: fc.name,
                    arguments: fc.arguments,
                })
                .collect();
        }
    }
    Vec::new()
}
