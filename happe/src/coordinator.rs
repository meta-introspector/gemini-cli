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
    session: &mut Session,
    query: String,
) -> Result<String> {
    // Get conversation history from the session data
    let history_contents = get_conversation_history(session); // Now Vec<Content>
    
    // Get recent conversation context for memory retrieval (last 3 turns)
    // let conversation_context = extract_conversation_context(&history, 3); // Commented out - history type changed
    let conversation_context = None; // Or retrieve differently if needed by IDA
    
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

    // Construct the parts for the current query + memories
    let current_query_parts = construct_prompt_parts(&query, &memories);
    let current_query_content = Content {
        parts: current_query_parts,
        role: Some("user".to_string()),
    };
    
    // Combine history with the current query content for the first LLM call
    let mut initial_llm_contents = history_contents.clone();
    initial_llm_contents.push(current_query_content.clone()); // Clone query content for history tracking

    // Log the user query part specifically
    debug!(query = query, "Constructed prompt content including history");

    // 4. Call LLM with the prompt
    // Clone contents here so the original can be used for the tool loop history
    let initial_contents_for_llm = initial_llm_contents.clone(); // Clone the combined history+query
    let (response_text, function_calls) = match llm_client::generate_response(
        gemini_client,
        initial_contents_for_llm, // Pass the clone
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

    // 6. Handle any function calls from the LLM
    let mut final_response = response_text;
    // Initialize current_contents for the tool loop with combined history+query
    let mut current_contents = initial_llm_contents; 

    // Add the initial model response (text + potential function call) to history
    if !final_response.is_empty() || !function_calls.is_empty() {
        let mut model_parts = vec![];
        if !final_response.is_empty() {
            model_parts.push(Part::text(final_response.clone()));
        }
        // Convert gemini_mcp::gemini::FunctionCall to gemini_core::types::FunctionCall for history
        for fc in &function_calls {
            model_parts.push(Part {
                text: None,
                function_call: Some(gemini_core::types::FunctionCall {
                    name: fc.name.clone(),
                    arguments: fc.arguments.clone(),
                }),
                function_response: None,
            });
        }
        if !model_parts.is_empty() {
            current_contents.push(Content {
                parts: model_parts,
                role: Some("model".to_string()),
            });
        }
    }

    // Loop in case of sequential function calls
    let mut function_call_iterations = 0;
    let max_iterations = 5; // Limit sequential calls

    let mut current_function_calls = function_calls; // Use the calls from the first response

    while !current_function_calls.is_empty() && function_call_iterations < max_iterations {
        function_call_iterations += 1;
        debug!(
            iteration = function_call_iterations,
            count = current_function_calls.len(),
            "Processing function calls iteration"
        );

        let mut tool_results_parts: Vec<Part> = vec![];

        for function_call in current_function_calls {
            // Extract server and tool names from the function call name
            // Allow both formats: 'server.tool_name' and 'tool_name'
            // Also handle cases like 'command__mcp_execute_command' which should map to 'command-mcp'/'execute_command'
            let (server_name, tool_name) = if function_call.name.contains('.') {
                // Format is server.tool_name
                let parts: Vec<&str> = function_call.name.splitn(2, '.').collect();
                (parts[0].to_string(), parts[1].to_string())
            } else if function_call.name.contains("__mcp_") {
                // Format is server__mcp_tool_name (where server is command, filesystem, etc.)
                let parts: Vec<&str> = function_call.name.splitn(2, "__mcp_").collect();
                (format!("{}-mcp", parts[0]), parts[1].to_string())
            } else {
                // Format is just tool_name, try to infer server from first part
                // Default to "command-mcp" for simple commands
                let server = if function_call.name.starts_with("execute_") {
                    "command-mcp".to_string()
                } else if function_call.name.contains("file") || function_call.name.contains("directory") {
                    "filesystem-mcp".to_string()
                } else if function_call.name.contains("memory") {
                    "memory-store-mcp".to_string()
                } else {
                    // Log the error but still try with command-mcp as fallback
                    warn!(name = function_call.name, "Unrecognized function call format, using command-mcp as fallback");
                    "command-mcp".to_string()
                };
                (server, function_call.name.clone())
            };

            info!(server = server_name, tool = tool_name, "Executing tool");

            match mcp_client
                .execute_tool(&server_name, &tool_name, function_call.arguments) // Use original arguments
                .await
            {
                Ok(result) => {
                    debug!(result = ?result, "Tool execution succeeded");
                    // Add the successful tool result part for the LLM using the constructor
                     tool_results_parts.push(
                         Part::function_response(
                             function_call.name.clone(), // Use original function name
                             result // Result is already serde_json::Value
                         )
                     );
                }
                Err(e) => {
                    error!(error = %e, "Tool execution failed");
                    // Add an error result part for the LLM using the constructor
                    tool_results_parts.push(
                        Part::function_response(
                             function_call.name.clone(),
                             serde_json::json!({ "error": format!("Tool execution failed: {}", e) })
                        )
                     );
                }
            }
        }

        // Add all tool results as a single 'function' role content part
        if !tool_results_parts.is_empty() {
             current_contents.push(Content {
                parts: tool_results_parts,
                role: Some("function".to_string()), // Use "function" role for tool results
            });
        } else {
             // No tool results were generated, break the loop
             break;
        }


        // Call LLM again with the updated history (including tool results)
        debug!("Sending tool results back to LLM");
        let (next_response_text, next_function_calls) = match llm_client::generate_response(
            gemini_client,
            current_contents.clone(), // Pass the updated history
            &system_prompt, // Pass as slice
            tools.as_deref(),
        )
        .await
        {
            Ok(resp) => resp,
            Err(e) => {
                error!(error = %e, "Failed to get follow-up response from LLM after tool call");
                // Use the last successful text response or an error message
                final_response = format!("{}\n\nError getting response after tool execution: {}", final_response, e);
                current_function_calls = vec![]; // Stop looping
                break;
            }
        };

        // Update final response and prepare for potential next iteration
        final_response = next_response_text;
        current_function_calls = next_function_calls; // Check if the LLM wants another tool call

        // Add the model's response (text + potential function calls) to history for the *next* iteration (if any)
        if !final_response.is_empty() || !current_function_calls.is_empty() {
            let mut model_parts = vec![];
            if !final_response.is_empty() {
                 // Use the constructor
                model_parts.push(Part::text(final_response.clone()));
            }
             // Convert gemini_mcp::gemini::FunctionCall to gemini_core::types::FunctionCall for history
            for fc in &current_function_calls {
                 // Construct manually
                 model_parts.push(Part {
                    text: None,
                    function_call: Some(gemini_core::types::FunctionCall {
                        name: fc.name.clone(),
                        arguments: fc.arguments.clone(),
                    }),
                     function_response: None,
                });
            }
             if !model_parts.is_empty() {
                current_contents.push(Content {
                    parts: model_parts,
                    role: Some("model".to_string()),
                });
            }
        }

    } // End while loop for function calls

    if function_call_iterations >= max_iterations {
        warn!("Reached maximum function call iterations ({})", max_iterations);
        // Append a warning to the final response?
        final_response = format!("{}\n\nWarning: Reached maximum sequential tool call limit.", final_response);
    }

    // 5. Store the completed conversation turn asynchronously (Moved here)
    // `current_contents` now holds the full history up to the final response/call
    // The parts for *this turn* are those in `current_contents` after the initial `history_contents` length
    let turn_specific_parts = if current_contents.len() > history_contents.len() {
        current_contents[history_contents.len()..].to_vec()
    } else {
        // This case might occur if the initial LLM call failed AND no function calls were made
        warn!("No new content parts generated for the turn? Final response: {}", final_response);
        // Store at least the user query part and maybe an error response part?
        // For now, store empty, but might need refinement.
        vec![]
    };

    let turn_data = ConversationTurn {
        user_query: query.to_string(), // Store the original query
        retrieved_memories: memories.clone(), // Store the memories used
        llm_response: final_response.clone(), // Store the final text response
        turn_parts: turn_specific_parts, // Store the parts specific to this turn
    };

    // Use the static method to store the turn
    if let Err(e) = IdaClient::store_turn_async(
        &ida_socket_path_str,
        turn_data.clone(), // Clone data for async task
    )
    .await
    {
        warn!(error = %e, "Failed to send turn data to IDA for storage");
        // Continue despite error, the main response is still processed
    }

    // Update session history internally before returning
    update_session_history(session, turn_data.clone()); // Pass &mut session

    Ok(final_response)
}

/// Get conversation history from the session data
fn get_conversation_history(session: &Session) -> Vec<Content> {
    let history_str = match session.get("conversation_history") {
        Some(history_json) => history_json,
        None => return Vec::new(),
    };
    
    // Deserialize into the old format
    let history_turns: Vec<ConversationTurn> = serde_json::from_str(history_str).unwrap_or_else(|e| {
        warn!(error = %e, "Failed to parse conversation history from session, returning empty history");
        Vec::new()
    });

    // Extract and flatten the turn_parts
    history_turns.into_iter().flat_map(|turn| turn.turn_parts).collect()
}

/// Update the session with new conversation turn
pub fn update_session_history(session: &mut Session, turn: ConversationTurn) {
    // Get existing history (still Vec<ConversationTurn> for now, IDA handles storage)
    let mut history: Vec<ConversationTurn> = match session.get("conversation_history") {
        Some(history_json) => serde_json::from_str(history_json).unwrap_or_default(),
        None => Vec::new(),
    };
    
    // Add new turn
    history.push(turn);
    
    // Limit history size if needed (keep last 10 turns)
    // Note: This limit is now on *turns*, not individual Content parts
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

// Construct Content parts for the current query + memories
// History will be prepended separately
fn construct_prompt_parts(query: &str, memories: &[MemoryItem]) -> Vec<Part> {
    let mut parts = Vec::new();

    // Add memory context if available
    if !memories.is_empty() {
        let mut memory_text = String::new();
        memory_text.push_str("Relevant previous interactions:\n");

        for (i, mem) in memories.iter().enumerate() {
            memory_text.push_str(&format!("{}. {}\n", i + 1, mem.content));
        }

        memory_text.push('\n');
        // Add memories as text within the user query part
        parts.push(Part::text(memory_text)); 
    }

    // Add the current query text
    parts.push(Part::text(query.to_string()));

    parts
}

// Create helper function to extract conversation context from history
// This might be less useful now that we pass full history Content
// Keeping it for now in case IDA needs it.
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
