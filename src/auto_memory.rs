use crate::logging::{log_debug, log_error};
use crate::mcp::host::McpHost;
use reqwest::Client;
use serde_json::{json, Value};
use std::error::Error;

/// Extracts key information from query and response using a flash model
pub async fn extract_key_information(
    query: &str,
    response: &str,
    api_key: &str,
    model: &str
) -> Result<Vec<Value>, Box<dyn Error>> {
    // Create prompt to extract key information
    let extraction_prompt = format!(
        "Extract key pieces of information from this conversation that would be valuable to remember for future interactions.\n\nUSER QUERY:\n\"{}\"\n\nASSISTANT RESPONSE:\n\"{}\"\n\nFor each piece of key information, return a JSON object with these properties:\n- key: A brief, descriptive identifier for this information (e.g., 'project_deadline', 'user_preference_theme')\n- value: The specific information to remember\n- tags: An array of 1-3 categorical tags (e.g., ['project', 'timeline'], ['preference', 'ui'])\n\nReturn these as a JSON array of objects. If no key information is found, return an empty array.\nOnly extract truly important/reusable information. Focus on facts, preferences, project details.\n\nReturn ONLY valid JSON, no other text.",
        query,
        response
    );
    
    // Call the LLM to extract information
    let client = Client::new();
    let url = format!("https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent", model);
    
    let request_body = json!({
        "contents": [{
            "role": "user",
            "parts": [{
                "text": extraction_prompt
            }]
        }]
    });
    
    let response = client
        .post(&format!("{}?key={}", url, api_key))
        .json(&request_body)
        .send()
        .await?;
    
    if !response.status().is_success() {
        let error_text = response.text().await?;
        log_error(&format!("Failed to extract memories: {}", error_text));
        return Ok(Vec::new());
    }
    
    let response_json: Value = response.json().await?;
    
    // Extract the generated content
    if let Some(text) = response_json["candidates"][0]["content"]["parts"][0]["text"].as_str() {
        // Parse the JSON response
        let memories: Vec<Value> = match serde_json::from_str(text) {
            Ok(memories) => memories,
            Err(e) => {
                // Try to extract JSON if there's surrounding text
                if let Some(start) = text.find('[') {
                    if let Some(end) = text.rfind(']') {
                        let json_str = &text[start..=end];
                        match serde_json::from_str(json_str) {
                            Ok(memories) => memories,
                            Err(_) => {
                                log_error(&format!("Failed to parse extracted memories: {}", e));
                                return Ok(Vec::new());
                            }
                        }
                    } else {
                        log_error(&format!("Failed to find JSON array end in response: {}", e));
                        return Ok(Vec::new());
                    }
                } else {
                    log_error(&format!("Failed to find JSON array start in response: {}", e));
                    return Ok(Vec::new());
                }
            }
        };
        
        Ok(memories)
    } else {
        Ok(Vec::new())
    }
}

/// Stores memories in the memory store MCP server
pub async fn store_memories(
    memories: Vec<Value>,
    mcp_host: &McpHost
) -> Result<(), Box<dyn Error>> {
    if memories.is_empty() {
        return Ok(());
    }
    
    // Check if memory server is available
    let capabilities = mcp_host.get_all_capabilities().await;
    
    // Find memory server with store_memory tool
    let memory_server = capabilities.tools.iter()
        .find(|tool| tool.name.contains("/store_memory"))
        .map(|tool| {
            let parts: Vec<&str> = tool.name.split('/').collect();
            (parts[0], parts[1])
        });
    
    if let Some((server_name, tool_name)) = memory_server {
        log_debug(&format!("Using memory server '{}' with tool '{}'", server_name, tool_name));
        
        // Store each memory using the memory MCP server
        for memory in memories {
            if let (Some(key), Some(value)) = (memory.get("key").and_then(|k| k.as_str()), 
                                               memory.get("value").and_then(|v| v.as_str())) {
                // Get tags if available
                let tags = memory.get("tags").and_then(|t| t.as_array())
                    .map(|arr| arr.iter()
                        .filter_map(|tag| tag.as_str().map(String::from))
                        .collect::<Vec<String>>())
                    .unwrap_or_else(Vec::new);
                
                // Store the memory
                let result = mcp_host.execute_tool(
                    server_name,
                    tool_name,
                    json!({
                        "key": key,
                        "value": value,
                        "tags": tags
                    }),
                ).await;
                
                match result {
                    Ok(_) => log_debug(&format!("Stored memory: {} = {}", key, value)),
                    Err(e) => log_error(&format!("Failed to store memory: {}", e))
                }
            }
        }
        
        Ok(())
    } else {
        log_debug("No memory server with store_memory tool found");
        Ok(())
    }
} 