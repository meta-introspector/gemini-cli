// Imports needed by the moved logic
use crate::cli::Args;
use crate::config::AppConfig;
use crate::history::{
    ChatHistory, ChatMessage, roles,
    load_chat_history, save_chat_history,
    start_new_chat,
    create_system_prompt_with_history,
    estimate_total_tokens, summarize_conversation,
    log_history_debug, TOKEN_THRESHOLD,
};
use crate::logging::{log_debug, log_error, log_info, log_warning};
use crate::mcp::host::McpHost;
use crate::mcp::gemini::{build_mcp_system_prompt, generate_gemini_function_declarations, process_function_call};
use crate::model::{call_gemini_api, send_function_response};
use crate::output::{print_gemini_response, handle_command_confirmation};
use crate::memory_broker;
use crate::auto_memory;

use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use std::env;
use std::error::Error;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Processes the user's prompt and interacts with the Gemini API.
pub async fn process_prompt(
    args: &Args,
    config: &AppConfig,
    client: &Client,
    mcp_host: &Option<McpHost>,
    api_key: &str,
    system_prompt: &str,
    config_dir: &Path,
    session_id: &str,
    should_save_history: bool,
    prompt: &str,
) -> Result<(), Box<dyn Error>> {
    // Start a new chat or use existing history
    let mut chat_history = if should_save_history && !args.new_chat {
        load_chat_history(&config_dir, &session_id)
    } else {
        // If history is disabled or new chat requested, start with an empty one
        if args.new_chat && should_save_history {
            // Clear existing history when explicitly starting a new chat
            start_new_chat(&config_dir, &session_id);
        }
        ChatHistory { messages: Vec::new(), session_id: session_id.to_string() }
    };

    let mut mcp_capabilities_prompt = String::new();
    let mut function_definitions = None; // For function calling
    
    // Get MCP capabilities if host exists
    if let Some(host) = mcp_host {
        let capabilities = host.get_all_capabilities().await;
        if !capabilities.tools.is_empty() || !capabilities.resources.is_empty() {
            log_info(&format!("MCP Capabilities discovered: {} tools, {} resources", 
                    capabilities.tools.len(), capabilities.resources.len()));
            
            // Format capabilities for the prompt
            mcp_capabilities_prompt = build_mcp_system_prompt(&capabilities.tools, &capabilities.resources);
            
            // Generate function declarations for tools
            if !capabilities.tools.is_empty() {
                function_definitions = generate_gemini_function_declarations(&capabilities.tools);
            }
        }
    }
    
    // Combine the user's system prompt with MCP capabilities information
    let mut system_prompt_with_history = create_system_prompt_with_history(&system_prompt);
    if !mcp_capabilities_prompt.is_empty() {
        system_prompt_with_history.push_str("\n\n");
        system_prompt_with_history.push_str(&mcp_capabilities_prompt);
    }
    
    // Format user prompt based on flags
    let formatted_prompt = if args.command_help {
        format!("Provide the Linux command for: {}", prompt)
    } else {
        prompt.to_string()
    };
    
    // Enhance query with relevant memories if memory broker is enabled
    let enhanced_prompt = if config.enable_memory_broker.unwrap_or(true) && mcp_host.is_some() {
        log_debug("Memory broker is enabled, enhancing query with relevant memories");
        let memories = memory_broker::retrieve_all_memories(mcp_host.as_ref().unwrap()).await?;
        
        if !memories.is_empty() {
            log_debug(&format!("Retrieved {} memories from store", memories.len()));
            // Filter memories by relevance
            let model = config.memory_broker_model.as_deref().unwrap_or("gemini-2.0-flash");
            let relevant_memories = memory_broker::filter_relevant_memories(
                &formatted_prompt, 
                memories, 
                api_key,
                model
            ).await?;
            
            if !relevant_memories.is_empty() {
                log_debug(&format!("Found {} relevant memories for query", relevant_memories.len()));
                // Enhance query with relevant memories
                memory_broker::enhance_query(&formatted_prompt, relevant_memories).await
            } else {
                log_debug("No relevant memories found for query");
                formatted_prompt
            }
        } else {
            log_debug("No memories found in store");
            formatted_prompt
        }
    } else {
        formatted_prompt
    };
    
    // Call Gemini API
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(ProgressStyle::default_spinner()
        .tick_strings(&["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"])
        .template("{spinner:.green} {msg}")
        .unwrap());
    spinner.set_message("Asking Gemini...".to_string());
    spinner.enable_steady_tick(Duration::from_millis(80));
    
    // Add user message to history
    let user_message = ChatMessage {
        role: roles::USER.to_string(),
        content: enhanced_prompt.clone(),
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    };
    chat_history.messages.push(user_message);
    
    // Call API and handle response
    match call_gemini_api(
        client,
        api_key,
        Some(&system_prompt_with_history),
        &enhanced_prompt,
        Some(&chat_history),
        function_definitions.clone(),
    ).await {
        Ok((response, function_calls)) => {
            spinner.finish_and_clear();
            
            // Add assistant message to history
            let assistant_message = ChatMessage {
                role: roles::ASSISTANT.to_string(),
                content: response.clone(),
                timestamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            };
            chat_history.messages.push(assistant_message);
            
            // Store memory from response if auto memory is enabled
            if config.enable_auto_memory.unwrap_or(true) && mcp_host.is_some() {
                log_debug("Auto memory is enabled, extracting key information");
                if let Some(api_key_str) = &config.api_key {
                    let model = config.memory_broker_model.as_deref().unwrap_or("gemini-2.0-flash");
                    
                    // Extract key information from the conversation
                    match auto_memory::extract_key_information(
                        &enhanced_prompt, 
                        &response, 
                        api_key_str,
                        model
                    ).await {
                        Ok(memories) => {
                            if !memories.is_empty() {
                                log_debug(&format!("Extracted {} key memories from conversation", memories.len()));
                                // Store memories
                                if let Err(e) = auto_memory::store_memories(memories, mcp_host.as_ref().unwrap()).await {
                                    log_error(&format!("Failed to store memories: {}", e));
                                }
                            } else {
                                log_debug("No key information found to store as memories");
                            }
                        },
                        Err(e) => log_error(&format!("Failed to extract memories: {}", e))
                    }
                }
            }
            
            // Process any function calls
            for function_call in &function_calls {
                if let Some(host) = mcp_host {
                    match process_function_call(function_call, host).await {
                        Ok(result) => {
                            // Only display function result if success is false or in debug mode
                            let should_show_result = result.get("success")
                                .and_then(|v| v.as_bool())
                                .map_or(true, |success| !success || std::env::var("GEMINI_DEBUG").is_ok());
                            
                            if should_show_result {
                                println!("\n{}: {}", "Function result".cyan(), 
                                         serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()));
                            }
                            
                            // Store the original function result for history
                            let function_result_str = format!("Function '{}' executed successfully with result: {}", 
                                                        function_call.name, 
                                                        serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()));
                            
                            // Send the function result back to the model to get final answer - skip notification
                            let spinner = ProgressBar::new_spinner();
                            spinner.set_style(ProgressStyle::default_spinner()
                                .tick_strings(&["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"])
                                .template("{spinner:.green} {msg}")
                                .unwrap());
                            spinner.set_message("Generating response...".to_string());
                            spinner.enable_steady_tick(Duration::from_millis(80));
                            
                            match send_function_response(
                                client,
                                api_key,
                                Some(&system_prompt_with_history),
                                &enhanced_prompt, // Original user prompt
                                function_call,    // The function call made by the model
                                result.clone(),    // The result from function execution
                            ).await {
                                Ok(final_response) => {
                                    spinner.finish_and_clear();
                                    
                                    // First add the function execution result to history as a system message for context
                                    chat_history.messages.push(ChatMessage {
                                        role: roles::SYSTEM.to_string(),
                                        content: function_result_str,
                                        timestamp: SystemTime::now()
                                            .duration_since(UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_secs(),
                                    });
                                    
                                    // Then add the final model response to history
                                    chat_history.messages.push(ChatMessage {
                                        role: roles::ASSISTANT.to_string(),
                                        content: final_response.clone(),
                                        timestamp: SystemTime::now()
                                            .duration_since(UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_secs(),
                                    });
                                    
                                    // Display the final response
                                    if args.command_help {
                                        // Extract command from the final response if this was a command request
                                        let command = final_response.trim().replace("`", "");
                                        handle_command_confirmation(&command)?;
                                    } else {
                                        // Display normally
                                        print_gemini_response(&final_response, false);
                                    }
                                    
                                    // Store memories from the function call response
                                    if config.enable_auto_memory.unwrap_or(true) && mcp_host.is_some() {
                                        log_debug("Auto memory is enabled, extracting key information from function response");
                                        if let Some(api_key_str) = &config.api_key {
                                            let model = config.memory_broker_model.as_deref().unwrap_or("gemini-2.0-flash");
                                            
                                            // Extract key information from the conversation
                                            match auto_memory::extract_key_information(
                                                &enhanced_prompt, 
                                                &final_response, 
                                                api_key_str,
                                                model
                                            ).await {
                                                Ok(memories) => {
                                                    if !memories.is_empty() {
                                                        log_debug(&format!("Extracted {} key memories from function response", memories.len()));
                                                        // Store memories
                                                        if let Err(e) = auto_memory::store_memories(memories, mcp_host.as_ref().unwrap()).await {
                                                            log_error(&format!("Failed to store memories from function response: {}", e));
                                                        }
                                                    } else {
                                                        log_debug("No key information found in function response");
                                                    }
                                                },
                                                Err(e) => log_error(&format!("Failed to extract memories from function response: {}", e))
                                            }
                                        }
                                    }
                                },
                                Err(e) => {
                                    spinner.finish_and_clear();
                                    eprintln!("\n{}: {}", "Failed to get final response from model".red(), e);
                                    
                                    // Still add the function execution to history
                                    chat_history.messages.push(ChatMessage {
                                        role: roles::SYSTEM.to_string(),
                                        content: function_result_str,
                                        timestamp: std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_secs(),
                                    });
                                    
                                    // Add an error message about the final response
                                    let error_msg = format!("Failed to get final response from model after function execution: {}", e);
                                    chat_history.messages.push(ChatMessage {
                                        role: roles::SYSTEM.to_string(),
                                        content: error_msg,
                                        timestamp: std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_secs(),
                                    });
                                    
                                    // Continue with original response since we couldn't get the final one
                                    if args.command_help {
                                        let command = response.trim().replace("`", "");
                                        handle_command_confirmation(&command)?;
                                    } else {
                                        print_gemini_response(&response, false);
                                    }
                                }
                            }
                        },
                        Err(e) => {
                            eprintln!("\n{}: {}", "Function execution error".red(), e);
                            
                            // Add error to history
                            let error_msg = format!("Function '{}' execution failed: {}", function_call.name, e);
                            chat_history.messages.push(ChatMessage {
                                role: roles::SYSTEM.to_string(),
                                content: error_msg,
                                timestamp: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs(),
                            });
                        }
                    }
                }
            }
            
            // Regular response handling (only show if no function calls were processed)
            if function_calls.is_empty() {
                if args.command_help {
                    // Extract command from response
                    let command = response.trim().replace("`", "");
                    handle_command_confirmation(&command)?;
                } else {
                    // Display normally
                    print_gemini_response(&response, false);
                }
                
                // Store memories from the conversation
                if config.enable_auto_memory.unwrap_or(true) && mcp_host.is_some() {
                    log_debug("Auto memory is enabled, extracting key information");
                    if let Some(api_key_str) = &config.api_key {
                        let model = config.memory_broker_model.as_deref().unwrap_or("gemini-2.0-flash");
                        
                        // Extract key information from the conversation
                        match auto_memory::extract_key_information(
                            &enhanced_prompt, 
                            &response, 
                            api_key_str,
                            model
                        ).await {
                            Ok(memories) => {
                                if !memories.is_empty() {
                                    log_debug(&format!("Extracted {} key memories from conversation", memories.len()));
                                    // Store memories
                                    if let Err(e) = auto_memory::store_memories(memories, mcp_host.as_ref().unwrap()).await {
                                        log_error(&format!("Failed to store memories: {}", e));
                                    }
                                } else {
                                    log_debug("No key information found to store as memories");
                                }
                            },
                            Err(e) => log_error(&format!("Failed to extract memories: {}", e))
                        }
                    }
                }
            }
            
            // Debug logging of history if enabled
            if env::var("GEMINI_DEBUG").is_ok() {
                let token_count = estimate_total_tokens(&chat_history, &system_prompt);
                let was_summarized = chat_history.messages.iter().any(|msg| 
                    msg.role == roles::SYSTEM && msg.content.contains("summarized version of a longer conversation"));
                log_history_debug(&chat_history, token_count, was_summarized, &session_id);
            }
            
            // Save updated history if enabled
            if should_save_history {
                save_chat_history(&config_dir, &chat_history)?;
            }
            
            // Check if we need to summarize history
            if should_save_history && estimate_total_tokens(&chat_history, &system_prompt) > TOKEN_THRESHOLD {
                log_info("Chat history is getting long. Summarizing...");
                println!("\n{}", "Chat history is getting long. Summarizing...".cyan());
                match summarize_conversation(client, api_key, &chat_history).await {
                    Ok(new_history) => {
                        chat_history = new_history;
                        save_chat_history(&config_dir, &chat_history)?;
                        log_info("History summarized successfully");
                        println!("{}", "History summarized successfully.".green());
                    },
                    Err(e) => {
                        log_warning(&format!("Failed to summarize history: {}", e));
                        eprintln!("{}: {}", "Failed to summarize history".red(), e);
                    }
                }
            }
        },
        Err(e) => {
            spinner.finish_and_clear();
            eprintln!("{}: {}", "Error calling Gemini API".red(), e);
        }
    }
    Ok(())
}