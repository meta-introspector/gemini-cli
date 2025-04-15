use serde::{Serialize, Deserialize};
use std::error::Error;
use reqwest::Client;
use colored::*;
use log::debug;

use crate::history::ChatHistory;
use crate::mcp::gemini::FunctionCall;

// --- Structs for Gemini API Request/Response --- //
pub struct GeminiModel {
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

#[derive(Serialize)]
pub struct GenerateContentRequest {
    pub contents: Vec<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GenerationConfig>,
}

#[derive(Serialize)]
pub struct Tool {
    pub function_declarations: Vec<FunctionDeclaration>,
}

#[derive(Serialize)]
pub struct FunctionDeclaration {
    pub name: String,
    pub description: Option<String>,
    pub parameters: serde_json::Value,
}

#[derive(Serialize, Clone, Debug)]
pub struct Content {
    pub parts: Vec<Part>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct Part {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(rename = "functionCall", skip_serializing_if = "Option::is_none")]
    pub function_call: Option<crate::mcp::gemini::FunctionCall>,
    #[serde(rename = "functionResponse", skip_serializing_if = "Option::is_none")]
    pub function_response: Option<FunctionResponse>,
}

#[derive(Serialize, Clone, Debug)]
pub struct FunctionResponse {
    pub name: String,
    pub response: serde_json::Value,
}

impl Part {
    pub fn text(text: String) -> Self {
        Self {
            text: Some(text),
            function_call: None,
            function_response: None,
        }
    }

    pub fn function_response(name: String, response: serde_json::Value) -> Self {
        Self {
            text: None,
            function_call: None,
            function_response: Some(FunctionResponse { name, response }),
        }
    }
}

// Define structs mirroring the Gemini API response structure for deserialization
#[derive(Deserialize, Debug, Serialize)]
struct GenerateContentResponse {
    candidates: Vec<Candidate>,
}

#[derive(Deserialize, Debug, Serialize)]
struct Candidate {
    content: Option<ContentResponsePart>,
}

#[derive(Deserialize, Debug, Serialize)]
struct ContentResponsePart {
    parts: Vec<PartResponse>,
    role: Option<String>,
}

#[derive(Deserialize, Debug, Serialize)]
struct PartResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(rename = "functionCall", skip_serializing_if = "Option::is_none")]
    function_call: Option<crate::mcp::gemini::FunctionCall>,
}

#[derive(Serialize)]
pub struct GenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
}

// Create a helper function to use GeminiModel
fn get_gemini_model(api_key: String, model_name: Option<String>) -> GeminiModel {
    GeminiModel::new(api_key, model_name)
}

/// Call the Gemini API with the given parameters
pub async fn call_gemini_api(
    client: &Client,
    api_key: &str,
    system_prompt: Option<&str>,
    user_prompt: &str,
    chat_history: Option<&ChatHistory>,
    function_definitions: Option<Vec<crate::mcp::gemini::FunctionDef>>,
) -> Result<(String, Vec<FunctionCall>), Box<dyn Error>> {
    // Create model object
    let model = get_gemini_model(api_key.to_string(), None);
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        model.model_name,
        model.api_key
    );

    // Create system instruction if provided
    let system_instruction = system_prompt.map(|p| Content {
        parts: vec![Part::text(p.to_string())],
        role: Some("system".to_string()),
    });

    // Debug log for system instruction
    if std::env::var("GEMINI_DEBUG").is_ok() {
        if let Some(sys_instr) = &system_instruction {
            if let Some(text) = &sys_instr.parts.first().and_then(|p| p.text.as_ref()) {
                println!("{}: {}", "DEBUG".yellow(), &format!("System instruction is set with text length: {}", text.len()));
                println!("{}: {}", "DEBUG".yellow(), "System instruction first 100 chars: ");
                println!("{}: {}", "DEBUG".yellow(), &text.chars().take(100).collect::<String>());
                println!("{}: {}", "DEBUG".yellow(), &format!("System instruction role: {:?}", sys_instr.role));
            } else {
                println!("{}: {}", "DEBUG".yellow(), "System instruction has no text content!");
            }
        } else {
            println!("{}: {}", "DEBUG".yellow(), "No system instruction provided to API call!");
        }
    }
    
    // Convert chat history to content format expected by Gemini API
    let mut contents = Vec::new();
    
    // Add previous messages from history if available
    if let Some(history) = chat_history {
        for msg in &history.messages {
            // Skip system messages in the main content list if they exist
            if msg.role != "system" {
                // Map our internal roles to Gemini API roles
                let api_role = match msg.role.as_str() {
                    "user" => "user",
                    "assistant" => "model", // Map assistant to model
                    _ => continue, // Skip any other roles
                };
                
                contents.push(Content {
                    parts: vec![Part::text(msg.content.clone())],
                    role: Some(api_role.to_string()), // Use the mapped role
                });
            }
        }
    } else {
        // If no history, just add the current user prompt
         contents.push(Content {
            parts: vec![Part::text(user_prompt.to_string())],
            role: Some("user".to_string()), // Explicitly set role
        });
    }

    // Ensure the current user prompt is always the last message in the request `contents`
    // It might already be there if it was the last message added to history before calling
    // Remove the last message if it is identical to the current user prompt
    if let Some(last_msg) = contents.last() {
        if last_msg.role.as_deref() == Some("user") && 
           last_msg.parts.first().and_then(|p| p.text.as_ref()).map_or(false, |text| text == user_prompt) {
            contents.pop();
        }
    }
    // Add the current user prompt as the final part of the conversation
    contents.push(Content {
        parts: vec![Part::text(user_prompt.to_string())],
        role: Some("user".to_string()),
    });

    // Create the request body with function definitions if provided
    let mut request_body = GenerateContentRequest {
        contents,
        system_instruction,
        tools: None,
        generation_config: Some(GenerationConfig {
            temperature: Some(0.2), // Lower temperature for more predictable responses
        }),
    };
    
    // Add function calling definitions if provided
    // Clone the function_definitions to avoid borrowing issues
    let function_definitions_clone = function_definitions.clone();
    if let Some(function_defs) = function_definitions_clone {
        if !function_defs.is_empty() {
            let function_declarations = function_defs.into_iter()
                .map(|def| FunctionDeclaration {
                    name: def.name,
                    description: def.description,
                    parameters: def.parameters,
                })
                .collect();
            
            request_body.tools = Some(vec![Tool {
                function_declarations,
            }]);
        }
    }

    // Optional: Log the request body in debug mode
    if std::env::var("GEMINI_DEBUG").is_ok() {
         if let Ok(json_req) = serde_json::to_string_pretty(&request_body) {
            println!("{}", "\n--- API Request Body ---".purple());
            println!("{}", json_req);
            println!("{}", "------------------------".purple());
        }
    }

    let res = client
        .post(&url)
        .json(&request_body)
        .send()
        .await?;

    if res.status().is_success() {
        // Get the raw response string first for debugging
        let response_text = res.text().await?;
        
        // Optionally log raw response 
        if std::env::var("GEMINI_DEBUG").is_ok() {
            println!("{}", "\n--- Raw API Response ---".purple());
            println!("{}", response_text);
            println!("{}", "-----------------------".purple());
        }
        
        // Now parse it
        let response_body: GenerateContentResponse = serde_json::from_str(&response_text)
            .map_err(|e| format!("Failed to parse API response: {} - Raw response: {}", e, response_text))?;
        
        // Optional: Log the deserialized response body in debug mode
        if std::env::var("GEMINI_DEBUG").is_ok() {
            if let Ok(json_res) = serde_json::to_string_pretty(&response_body) {
                println!("{}", "\n--- API Response Body (Deserialized) ---".purple());
                println!("{}", json_res);
                println!("{}", "--------------------------------------".purple());
            }
        }

        let mut combined_text = String::new();
        let mut function_calls = Vec::new();

        // Process candidates (usually only one)
        if let Some(candidate) = response_body.candidates.first() {
            if let Some(content) = &candidate.content {
                // Log parts content for debugging function calls
                if std::env::var("GEMINI_DEBUG").is_ok() {
                    println!("{}", "\n--- Content Parts Information ---".purple());
                    println!("Number of parts: {}", content.parts.len());
                    for (i, part) in content.parts.iter().enumerate() {
                        println!("Part {}: text = {:?}, function_call = {:?}", 
                                i, part.text.as_ref().map(|t| t.len()), 
                                part.function_call.is_some());
                    }
                    println!("{}", "--------------------------------".purple());
                }
                
                for part in &content.parts {
                    // Append text if present
                    if let Some(text) = &part.text {
                        combined_text.push_str(text);
                    }
                    // Collect function calls if present
                    if let Some(call) = &part.function_call {
                        // We need to clone the FunctionCall since we're borrowing from response_body
                        function_calls.push(call.clone());
                    }
                }
            }
        }

        // Return combined text and any detected function calls
        if function_calls.is_empty() && combined_text.is_empty() {
            // Handle cases where the response might be empty or blocked
            debug!("Received empty response or potential block from API.");
            // Check for prompt feedback in the raw response if needed
            // For now, return empty text and no calls
            Ok((String::new(), Vec::new()))
        } else if function_calls.is_empty() && !combined_text.is_empty() && function_definitions.is_some() {
            // Fallback: If we expected function calls (function_definitions is Some) but didn't find any in the structured parts,
            // try parsing them from the text response as a last resort
            debug!("No structured function calls found, trying to parse from text response...");
            
            // Use the legacy function call parser as a fallback
            let parsed_calls = crate::mcp::gemini::parse_function_calls(&combined_text);
            if !parsed_calls.is_empty() {
                if std::env::var("GEMINI_DEBUG").is_ok() {
                    println!("{}", format!("\n--- Parsed {} fallback function calls from text ---", parsed_calls.len()).purple());
                    for (i, call) in parsed_calls.iter().enumerate() {
                        println!("{}. Function: {} with args: {:?}", i + 1, call.name.green(), call.arguments);
                    }
                }
                
                // Return the parsed calls with the text response
                Ok((combined_text, parsed_calls))
            } else {
                // No function calls found even with fallback parsing
                Ok((combined_text, Vec::new()))
            }
        } else {
            if !function_calls.is_empty() && std::env::var("GEMINI_DEBUG").is_ok() {
                println!("{}", format!("\n--- Parsed {} function calls from JSON ---", function_calls.len()).purple());
                for (i, call) in function_calls.iter().enumerate() {
                    // Use debug print for the args Value
                    println!("{}. Function: {} with args: {:?}", i + 1, call.name.green(), call.arguments);
                }
            }
            Ok((combined_text, function_calls))
        }
    } else {
        let status = res.status();
        let error_text = res.text().await.unwrap_or_else(|_| "Could not read error body".to_string());
        Err(format!("{} {}: {}", "API request failed with status".red(), status, error_text).into())
    }
}

/// Send a function response back to the model and get its final answer
pub async fn send_function_response(
    client: &Client,
    api_key: &str,
    system_prompt: Option<&str>,
    original_user_prompt: &str,
    function_call: &crate::mcp::gemini::FunctionCall,
    function_result: serde_json::Value,
    chat_history: Option<&ChatHistory>,
) -> Result<String, Box<dyn Error>> {
    // Create model object
    let model = get_gemini_model(api_key.to_string(), None);
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        model.model_name,
        model.api_key
    );

    // Create system instruction if provided
    let system_instruction = system_prompt.map(|p| Content {
        parts: vec![Part::text(p.to_string())],
        role: Some("system".to_string()),
    });

    // Create the conversation history
    let mut contents = Vec::new();
    
    // Add previous messages from history if available
    if let Some(history) = chat_history {
        for msg in &history.messages {
            // Skip system messages in the main content list
            if msg.role != "system" {
                // Map our internal roles to Gemini API roles
                let api_role = match msg.role.as_str() {
                    "user" => "user",
                    "assistant" => "model", // Map assistant to model
                    _ => continue, // Skip any other roles
                };
                
                contents.push(Content {
                    parts: vec![Part::text(msg.content.clone())],
                    role: Some(api_role.to_string()), // Use the mapped role
                });
            }
        }
    } else {
        // If no history, just add the current user prompt
        contents.push(Content {
            parts: vec![Part::text(original_user_prompt.to_string())],
            role: Some("user".to_string()),
        });
    }
    
    // Add the function call from the model if it's not already in the history
    // We need to clone the function_call to avoid ownership issues
    let function_call_clone = function_call.clone();
    let function_call_content = Content {
        parts: vec![Part {
            text: None,
            function_call: Some(function_call_clone),
            function_response: None,
        }],
        role: Some("model".to_string()), // Always use "model" for Gemini API
    };
    contents.push(function_call_content);
    
    // Process the function result to handle special case for command MCP server
    // Command results typically have a nested structure with stdout inside
    let processed_result = if function_call.name.starts_with("command.") {
        if let Some(inner_result) = function_result.get("result") {
            // Command results typically have stdout/stderr fields inside a result field
            inner_result.clone()
        } else {
            // If no nested result field, use as-is
            function_result.clone()
        }
    } else {
        // For non-command tools, use the original result
        function_result.clone()
    };
    
    // Log the processed result for debugging
    if std::env::var("DEBUG").is_ok() {
        println!("[DEBUG] Processed function result: {}", 
                serde_json::to_string_pretty(&processed_result)
                .unwrap_or_else(|_| processed_result.to_string()));
    }
    
    // Add the function execution result as functionResponse from user
    let function_response_part = Part::function_response(
        function_call.name.clone(),
        processed_result,
    );
    
    contents.push(Content {
        parts: vec![function_response_part],
        role: Some("user".to_string()),
    });

    // Create the request body
    let request_body = GenerateContentRequest {
        contents,
        system_instruction,
        tools: None, // No need to include tools when sending a function response
        generation_config: None,
    };
    
    // Optional: Log the request body in debug mode
    if std::env::var("GEMINI_DEBUG").is_ok() {
         if let Ok(json_req) = serde_json::to_string_pretty(&request_body) {
            println!("{}", "\n--- Function Response API Request Body ---".purple());
            println!("{}", json_req);
            println!("{}", "---------------------------------------".purple());
        }
    }

    // Send the request
    let res = client
        .post(&url)
        .json(&request_body)
        .send()
        .await?;

    if res.status().is_success() {
        // Deserialize the entire response using the response structs
        let response_body: GenerateContentResponse = res.json().await?;
        
        // Optional: Log the response body in debug mode
        if std::env::var("GEMINI_DEBUG").is_ok() {
            if let Ok(json_res) = serde_json::to_string_pretty(&response_body) {
                println!("{}", "\n--- Function Response API Result ---".purple());
                println!("{}", json_res);
                println!("{}", "------------------------------------".purple());
            }
        }

        let mut combined_text = String::new();

        // Extract the text response from candidates
        if let Some(candidate) = response_body.candidates.first() {
            if let Some(content) = &candidate.content {
                for part in &content.parts {
                    if let Some(text) = &part.text {
                        combined_text.push_str(text);
                    }
                }
            }
        }

        if combined_text.is_empty() {
            return Err("No text response received from model after function execution".into());
        }
        
        Ok(combined_text)
    } else {
        let status = res.status();
        let error_text = res.text().await.unwrap_or_else(|_| "Could not read error body".to_string());
        Err(format!("{} {}: {}", "Function response API request failed with status".red(), status, error_text).into())
    }
} 