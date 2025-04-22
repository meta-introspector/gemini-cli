use crate::ida_client::IdaClient;
use crate::llm_client;
use crate::mcp_client::{self, McpHostClient};
use crate::session::Session;
use anyhow::{anyhow, Result};
use gemini_core::client::GeminiClient;
use gemini_core::types::{Content, Part};
use gemini_ipc::internal_messages::{ConversationTurn, MemoryItem};
use gemini_mcp::gemini::build_mcp_system_prompt;
use tracing::{debug, error, info, warn};
use gemini_core::config::HappeConfig;

/// Process a single query from the user
pub async fn process_query(
    config: &HappeConfig,
    mcp_client: &McpHostClient,
    gemini_client: &GeminiClient,
    session: &Session,
    query: String,
) -> Result<String> {
    // Get conversation history from the session data
    let history = get_conversation_history(session);
    
    // Get recent conversation context for memory retrieval (last 3 turns)
    let conversation_context = extract_conversation_context(&history, 3);
    
    // Get relevant memories from IDA
    let ida_socket_path_str = config
        .ida_socket_path
        .as_ref()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "/tmp/gemini_suite_ida.sock".to_string());
        
    let memories = match IdaClient::get_memories(&ida_socket_path_str, &query, conversation_context).await {
        Ok(mem) => {
            info!(count = mem.len(), "Retrieved memories from IDA");
            mem
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

    let mcp_capabilities_prompt =
        build_mcp_system_prompt(&capabilities.tools, &capabilities.resources);

    let tools = if !capabilities.tools.is_empty() {
        Some(vec![mcp_client::generate_tool_declarations(
            &capabilities.tools,
        )])
    } else {
        None
    };

    // 3. Construct prompt with memories
    let base_system_prompt = config.system_prompt.as_deref().unwrap_or("You are a helpful assistant.");
    let system_prompt = format!("{}\n{}", base_system_prompt, mcp_capabilities_prompt);

    // Construct the prompt content including memories
    let contents = construct_prompt(&query, &memories);
    // Log the user query part specifically
    debug!(query = query, "Constructed prompt content");

    // 4. Call LLM with the prompt
    let (response_text, function_calls) = match llm_client::generate_response(
        gemini_client,
        contents, // Pass the constructed Vec<Content>
        &system_prompt,
        tools.as_deref(),
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
        &ida_socket_path_str,
        turn_data.clone(),
    )
    .await
    {
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

                info!(server = server_name, tool = tool_name, "Executing tool");

                match mcp_client
                    .execute_tool(server_name, tool_name, function_call.arguments)
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

                        final_response =
                            format!("{}\n\nFunction call result: {}", final_response, result_str);
                    }
                    Err(e) => {
                        error!(
                            error = %e,
                            "Tool execution failed"
                        );

                        final_response =
                            format!("{}\n\nFunction call error: {}", final_response, e);
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

/// Get conversation history from the session data
fn get_conversation_history(session: &Session) -> Vec<ConversationTurn> {
    let history_str = match session.get("conversation_history") {
        Some(history_json) => history_json,
        None => return Vec::new(),
    };
    
    serde_json::from_str(history_str).unwrap_or_else(|e| {
        warn!(error = %e, "Failed to parse conversation history from session, returning empty history");
        Vec::new()
    })
}

/// Update the session with new conversation turn
pub fn update_session_history(session: &mut Session, turn: ConversationTurn) {
    // Get existing history
    let mut history = get_conversation_history(session);
    
    // Add new turn
    history.push(turn);
    
    // Limit history size if needed (keep last 10 turns)
    if history.len() > 10 {
        history = history.drain(history.len() - 10..).collect();
    }
    
    // Serialize and save back to session
    match serde_json::to_string(&history) {
        Ok(history_json) => {
            session.set("conversation_history".to_string(), history_json);
        }
        Err(e) => {
            warn!(error = %e, "Failed to serialize conversation history");
        }
    }
}

// Improved prompt construction with memories
fn construct_prompt(query: &str, memories: &[MemoryItem]) -> Vec<Content> {
    let mut content_parts = Vec::new();

    // Add memory context if available
    if !memories.is_empty() {
        let mut memory_text = String::new();
        memory_text.push_str("Relevant previous interactions:\n");

        for (i, mem) in memories.iter().enumerate() {
            memory_text.push_str(&format!("{}. {}\n", i + 1, mem.content));
        }

        memory_text.push('\n');
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

    // Return the vector of Content directly
    content_parts
}

// Create helper function to extract conversation context from history
fn extract_conversation_context(history: &[ConversationTurn], max_turns: usize) -> Option<String> {
    if history.is_empty() {
        return None;
    }
    
    // Take at most the last max_turns from history
    let recent_turns = if history.len() > max_turns {
        &history[history.len() - max_turns..]
    } else {
        history
    };
    
    // Format the turns into a simple string representation
    let context = recent_turns
        .iter()
        .enumerate()
        .map(|(i, turn)| {
            format!(
                "Turn {}: User: {}\nAssistant: {}\n",
                i + 1,
                turn.user_query,
                turn.llm_response
            )
        })
        .collect::<Vec<String>>()
        .join("\n");
    
    if context.is_empty() {
        None
    } else {
        Some(context)
    }
}
