use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Helper struct to encapsulate model details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct GeminiModel {
    pub api_key: String,
    pub model_name: String,
}

impl GeminiModel {
    pub fn new(api_key: String, model_name: Option<String>) -> Self {
        Self {
            api_key,
            model_name: model_name.unwrap_or_else(|| "gemini-2.5-pro-preview-03-25".to_string()),
        }
    }
}

/// Function definition for tool calling
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FunctionDef {
    pub name: String,
    pub description: Option<String>,
    pub parameters: Value,
}

/// Function parameter definition
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FunctionParameter {
    #[serde(rename = "type")]
    pub param_type: String,
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, FunctionParameter>>,
}

/// Function call from LLM response
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FunctionCall {
    pub name: String,
    #[serde(rename = "args")]
    pub arguments: Value,
}

/// Function response to send back to LLM
#[derive(Serialize, Clone, Debug, Deserialize)]
pub struct FunctionResponse {
    pub name: String,
    pub response: Value,
}

/// Request to Gemini API to generate content
#[derive(Serialize, Debug)]
pub struct GenerateContentRequest {
    pub contents: Vec<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GenerationConfig>,
}

/// Tool definition for Gemini API
#[derive(Serialize, Debug, Clone)]
pub struct Tool {
    pub function_declarations: Vec<FunctionDeclaration>,
}

/// Function declaration for Gemini API
#[derive(Serialize, Debug, Clone)]
pub struct FunctionDeclaration {
    pub name: String,
    pub description: Option<String>,
    pub parameters: Value,
}

/// Content structure for requests and responses
#[derive(Serialize, Clone, Debug, Deserialize)]
pub struct Content {
    pub parts: Vec<Part>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

/// Part structure for a piece of content
#[derive(Serialize, Clone, Debug, Deserialize)]
pub struct Part {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(rename = "functionCall", skip_serializing_if = "Option::is_none")]
    pub function_call: Option<FunctionCall>,
    #[serde(rename = "functionResponse", skip_serializing_if = "Option::is_none")]
    pub function_response: Option<FunctionResponse>,
}

impl Part {
    pub fn text(text: String) -> Self {
        Self {
            text: Some(text),
            function_call: None,
            function_response: None,
        }
    }

    pub fn function_response(name: String, response: Value) -> Self {
        Self {
            text: None,
            function_call: None,
            function_response: Some(FunctionResponse { name, response }),
        }
    }
}

/// Generation configuration options
#[derive(Serialize, Debug, Default)]
pub struct GenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_mime_type: Option<String>,
}

/// Response from Gemini API
#[derive(Deserialize, Debug, Serialize)]
pub struct GenerateContentResponse {
    pub candidates: Vec<Candidate>,
}

/// Candidate in the response
#[derive(Deserialize, Debug, Serialize)]
pub struct Candidate {
    pub content: Option<ContentResponsePart>,
}

/// Content part in the response
#[derive(Deserialize, Debug, Serialize)]
pub struct ContentResponsePart {
    pub parts: Vec<PartResponse>,
    pub role: Option<String>,
}

/// Part response from the API
#[derive(Deserialize, Debug, Serialize)]
pub struct PartResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(rename = "functionCall", skip_serializing_if = "Option::is_none")]
    pub function_call: Option<FunctionCall>,
}
