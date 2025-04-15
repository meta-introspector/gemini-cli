use crate::logging::{log_debug, log_error};
use crate::mcp::host::McpHost;
use reqwest::Client;
use serde_json::{json, Value};
use std::error::Error;

/// Retrieves all memories from the memory store MCP server
pub async fn retrieve_all_memories(mcp_host: &McpHost) -> Result<Vec<Value>, Box<dyn Error>> {
    // Check if memory server is available
    let capabilities = mcp_host.get_all_capabilities().await;
    
    // Find memory server with list_all_memories tool
    let memory_server = capabilities.tools.iter()
        .find(|tool| tool.name.contains("/list_all_memories"))
        .map(|tool| {
            let parts: Vec<&str> = tool.name.split('/').collect();
            (parts[0], parts[1])
        });
    
    if let Some((server_name, tool_name)) = memory_server {
        log_debug(&format!("Using memory server '{}' with tool '{}'", server_name, tool_name));
        
        // Get all memories from the memory MCP server
        let response = mcp_host.execute_tool(
            server_name,
            tool_name,
            json!({}),
        ).await?;
        
        // Extract and return memories
        if let Some(memories) = response.get("memories").and_then(|m| m.as_array()) {
            return Ok(memories.clone());
        }
        
        log_debug("No memories found in memory store");
        Ok(Vec::new())
    } else {
        log_debug("No memory server with list_all_memories tool found");
        Ok(Vec::new())
    }
}

/// Filters memories by relevance to the given query using the specified model
pub async fn filter_relevant_memories(
    query: &str,
    memories: Vec<Value>,
    api_key: &str,
    model: &str
) -> Result<Vec<Value>, Box<dyn Error>> {
    if memories.is_empty() {
        return Ok(Vec::new());
    }
    
    // Format memories for the prompt
    let formatted_memories = memories.iter()
        .filter_map(|m| {
            let key = m.get("key").and_then(|k| k.as_str())?;
            let value = m.get("value").and_then(|v| v.as_str())?;
            Some(format!("- {} = {}", key, value))
        })
        .collect::<Vec<String>>()
        .join("\n");
    
    // Create prompt to filter relevant memories
    let prompt = format!(
        "Given a user query and a list of memory items, determine which memories (if any) are RELEVANT to answering the query. Return the indices of relevant memories ONLY in JSON format.\n\nQUERY: \"{}\"\n\nMEMORIES:\n{}\n\nRespond with JUST a JSON array of integers representing the 0-based indices of relevant memories. If no memories are relevant, return an empty array [].",
        query,
        formatted_memories
    );
    
    // Call the LLM to filter memories
    let client = Client::new();
    let url = format!("https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent", model);
    
    let request_body = json!({
        "contents": [{
            "role": "user",
            "parts": [{
                "text": prompt
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
        log_error(&format!("Failed to filter memories: {}", error_text));
        return Ok(Vec::new());
    }
    
    let response_json: Value = response.json().await?;
    
    // Extract the generated content
    if let Some(text) = response_json["candidates"][0]["content"]["parts"][0]["text"].as_str() {
        // Parse the JSON response to get indices
        let indices: Vec<usize> = match serde_json::from_str(text) {
            Ok(indices) => indices,
            Err(e) => {
                // Try to extract JSON if there's surrounding text
                if let Some(start) = text.find('[') {
                    if let Some(end) = text.rfind(']') {
                        let json_str = &text[start..=end];
                        match serde_json::from_str(json_str) {
                            Ok(indices) => indices,
                            Err(_) => {
                                log_error(&format!("Failed to parse memory indices: {}", e));
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
        
        // Return only the relevant memories
        let relevant_memories = indices.into_iter()
            .filter_map(|i| memories.get(i).cloned())
            .collect();
        
        return Ok(relevant_memories);
    }
    
    Ok(Vec::new())
}

/// Enhances a query with relevant memories to provide better context
pub async fn enhance_query(
    original_query: &str,
    relevant_memories: Vec<Value>
) -> String {
    if relevant_memories.is_empty() {
        return original_query.to_string();
    }
    
    // Format the memories for the model
    let formatted_memories = relevant_memories.iter()
        .filter_map(|m| {
            let key = m.get("key").and_then(|k| k.as_str())?;
            let value = m.get("value").and_then(|v| v.as_str())?;
            Some(format!("- {} = {}", key, value))
        })
        .collect::<Vec<String>>()
        .join("\n");
    
    // Create enhanced query with memory context
    format!(
        "I have some information stored in my memory that might be relevant to your query:\n\nRELEVANT MEMORIES:\n{}\n\nNow, responding to your query: {}",
        formatted_memories,
        original_query
    )
} 