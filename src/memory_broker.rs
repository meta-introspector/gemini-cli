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
    
    // Log the number of memories being processed
    log_debug(&format!("Processing {} memories for relevance filtering", memories.len()));
    
    // Format memories for the prompt - include key, value, and tags for better context
    let formatted_memories = memories.iter()
        .enumerate()
        .filter_map(|(i, m)| {
            let key = m.get("key").and_then(|k| k.as_str())?;
            let value = m.get("value").and_then(|v| v.as_str())?;
            let tags = m.get("tags")
                .and_then(|t| t.as_array())
                .map(|arr| arr.iter()
                    .filter_map(|tag| tag.as_str())
                    .collect::<Vec<&str>>()
                    .join(", "))
                .unwrap_or_default();
            
            Some(format!("{}: key=\"{}\" tags=[{}] value=\"{}\"", i, key, tags, value))
        })
        .collect::<Vec<String>>()
        .join("\n");
    
    // Create prompt to filter relevant memories with clear instructions
    let prompt = format!(
        "Given a user query and a list of memory items, determine which memories (if any) are RELEVANT to answering the query. Return the indices of relevant memories in JSON format.\n\nQUERY: \"{}\"\n\nMEMORIES:\n{}\n\nYour task:\n1. Analyze each memory carefully for relevance to the query\n2. Consider both direct and indirect relevance\n3. A memory is relevant if it would help answer the query or provide important context\n4. Respond with JUST a JSON array of integers representing the 0-based indices of relevant memories\n5. If no memories are relevant, return an empty array []\n\nJSON Response:",
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
        }],
        "generationConfig": {
            "temperature": 0.1  // Use lower temperature for more deterministic results
        }
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
            .collect::<Vec<Value>>();
        
        log_debug(&format!("Selected {} relevant memories out of {}", relevant_memories.len(), memories.len()));
        
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
    
    // Format the memories for the model in a more structured way
    let formatted_memories = relevant_memories.iter()
        .filter_map(|m| {
            let key = m.get("key").and_then(|k| k.as_str())?;
            let value = m.get("value").and_then(|v| v.as_str())?;
            let tags = m.get("tags")
                .and_then(|t| t.as_array())
                .map(|arr| arr.iter()
                    .filter_map(|tag| tag.as_str())
                    .collect::<Vec<&str>>()
                    .join(", "))
                .unwrap_or_else(|| "".to_string());
            
            Some(format!("• {} = {} [tags: {}]", key, value, tags))
        })
        .collect::<Vec<String>>()
        .join("\n");
    
    // Create enhanced query with memory context in a clearer format
    format!(
        "I have the following information from my memory that may be relevant to your query:\n\n```\n{}\n```\n\nBased on this context, I'll now answer your query: {}", 
        formatted_memories,
        original_query
    )
}

/// Deduplicates memories in the memory store by calling the deduplicate_memories tool
pub async fn deduplicate_memories(mcp_host: &McpHost) -> Result<(usize, usize), Box<dyn Error>> {
    // Check if memory server is available
    let capabilities = mcp_host.get_all_capabilities().await;
    
    // Find memory server with deduplicate_memories tool
    let memory_server = capabilities.tools.iter()
        .find(|tool| tool.name.contains("/deduplicate_memories"))
        .map(|tool| {
            let parts: Vec<&str> = tool.name.split('/').collect();
            (parts[0], parts[1])
        });
    
    if let Some((server_name, tool_name)) = memory_server {
        log_debug(&format!("Using memory server '{}' with tool '{}'", server_name, tool_name));
        
        // Call the deduplicate_memories tool
        let response = mcp_host.execute_tool(
            server_name,
            tool_name,
            json!({}),
        ).await?;
        
        // Extract the result
        let removed_count = response.get("removed_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        
        let remaining_count = response.get("remaining_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        
        log_debug(&format!("Deduplicated memories: removed {}, remaining {}", 
                          removed_count, remaining_count));
        
        Ok((removed_count, remaining_count))
    } else {
        log_debug("No memory server with deduplicate_memories tool found");
        Ok((0, 0))
    }
} 