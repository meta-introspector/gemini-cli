use crate::config::AppConfig;
use crate::ida_client::IdaClient;
use crate::llm_client;
use crate::mcp_client::{self, McpHostClient};
use anyhow::{anyhow, Result};
use gemini_core::client::GeminiClient;
use gemini_core::types::{Content, Part};
use gemini_ipc::internal_messages::{ConversationTurn, MemoryItem};
use gemini_mcp::gemini::build_mcp_system_prompt;
use tracing::{debug, error, info, warn};

/// Process a single query from the user
pub async fn process_query(
    config: &AppConfig,
    mcp_client: &McpHostClient,
    gemini_client: &GeminiClient,
    query: String,
) -> Result<String> {
    // 1. Get memories from IDA (Connects, gets, disconnects)
    let memories = match IdaClient::get_memories(
        config.ida_socket_path.to_str().unwrap_or_default(),
        &query
    ).await {
        Ok(m) => {
            info!(count = m.len(), "Retrieved memories from IDA");
            m
        }
        Err(e) => {
            warn!(error = %e, "Failed to retrieve memories from IDA, continuing without memories");
            vec![]
        }
    };

    // 2. Get MCP capabilities and build tool declarations
    let capabilities = match mcp_client.get_capabilities().await {
        Ok(caps) => caps,
        Err(e) => {
            warn!(error = %e, "Failed to get MCP capabilities, continuing without tools");
            gemini_core::rpc_types::ServerCapabilities::default()
        }
    };
    
    let mcp_capabilities_prompt = build_mcp_system_prompt(&capabilities.tools, &capabilities.resources);
    
    let tools = if !capabilities.tools.is_empty() {
        Some(vec![mcp_client::generate_tool_declarations(&capabilities.tools)])
    } else {
        None
    };

    // 3. Construct prompt with memories
    let system_prompt = format!(
        "{}\n{}",
        config.system_prompt,
        mcp_capabilities_prompt
    );
    
    let formatted_prompt = construct_prompt(&query, &memories);
    debug!(prompt = formatted_prompt, "Constructed prompt");

    // 4. Call LLM with the prompt
    let (response_text, function_calls) = match llm_client::generate_response(
        gemini_client,
        &formatted_prompt,
        &system_prompt,
        tools.as_ref().map(|v| &**v),
    )
    .await
    {
        Ok(resp) => resp,
        Err(e) => {
            error!(error = %e, "Failed to get response from LLM");
            return Err(anyhow!("Failed to get response from LLM: {}", e));
        }
    };

    // 5. Store the conversation turn asynchronously
    let turn_data = ConversationTurn {
        user_query: query.to_string(),
        retrieved_memories: memories.clone(),
        llm_response: response_text.clone(),
        // Add other fields as needed
    };
    
    // Use the static method to store the turn
    if let Err(e) = IdaClient::store_turn_async(
        config.ida_socket_path.to_str().unwrap_or_default(), 
        turn_data
    ).await {
        warn!(error = %e, "Failed to send turn data to IDA for storage");
        // Continue despite error
    }

    // 6. Handle any function calls from the LLM
    let mut final_response = response_text;
    
    if !function_calls.is_empty() {
        debug!(
            function_calls_count = function_calls.len(),
            "Processing function calls"
        );
        
        for function_call in function_calls {
            // Extract server and tool names
            let qualified_name = function_call.name.replace(".", "/");
            let parts: Vec<&str> = qualified_name.splitn(2, "/").collect();
            
            if parts.len() == 2 {
                let server_name = parts[0];
                let tool_name = parts[1];
                
                info!(
                    server = server_name,
                    tool = tool_name,
                    "Executing tool"
                );
                
                match mcp_client.execute_tool(
                    server_name,
                    tool_name,
                    function_call.arguments,
                )
                .await
                {
                    Ok(result) => {
                        debug!(
                            result = ?result,
                            "Tool execution succeeded"
                        );
                        
                        // For simplicity, we're adding the tool result to the response
                        // In a more robust implementation, we'd send it back to the LLM for further processing
                        let result_str = serde_json::to_string_pretty(&result)
                            .unwrap_or_else(|_| result.to_string());
                        
                        final_response = format!(
                            "{}\n\nFunction call result: {}",
                            final_response, result_str
                        );
                    }
                    Err(e) => {
                        error!(
                            error = %e,
                            "Tool execution failed"
                        );
                        
                        final_response = format!(
                            "{}\n\nFunction call error: {}",
                            final_response, e
                        );
                    }
                }
            } else {
                warn!(
                    name = function_call.name,
                    "Invalid function call name format"
                );
            }
        }
    }

    Ok(final_response)
}

// Improved prompt construction with memories
fn construct_prompt(query: &str, memories: &[MemoryItem]) -> String {
    let mut content_parts = Vec::new();
    
    // Add memory context if available
    if !memories.is_empty() {
        let mut memory_text = String::new();
        memory_text.push_str("Relevant previous interactions:\n");
        
        for (i, mem) in memories.iter().enumerate() {
            memory_text.push_str(&format!("{}. {}\n", i + 1, mem.content));
        }
        
        memory_text.push_str("\n");
        content_parts.push(Content {
            parts: vec![Part::text(memory_text)],
            role: Some("user".to_string()),
        });
    }
    
    // Add the current query
    content_parts.push(Content {
        parts: vec![Part::text(query.to_string())],
        role: Some("user".to_string()),
    });
    
    // Convert to the format expected by generate_response
    format!("{}", query)
}
