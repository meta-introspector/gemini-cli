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
use std::io::{self, Write};
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
    
    // Check if this is a query that's likely targeting memory MCP tools directly
    let is_memory_tool_query = prompt.to_lowercase().contains("memory") && 
        (prompt.to_lowercase().contains("store") || 
         prompt.to_lowercase().contains("list") || 
         prompt.to_lowercase().contains("update") || 
         prompt.to_lowercase().contains("delete") || 
         prompt.to_lowercase().contains("retrieve") ||
         prompt.to_lowercase().contains("deduplicate"));
    
    // Enhance query with relevant memories if memory broker is enabled and not directly using memory tools
    let enhanced_prompt = if config.enable_memory_broker.unwrap_or(true) && mcp_host.is_some() && !is_memory_tool_query {
        log_debug("Memory broker is enabled, enhancing query with relevant memories");
        
        // First, deduplicate existing memories to maintain a clean memory store
        // Do this occasionally to avoid overhead on every query
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        // Deduplicate every ~10 queries (deterministic based on time)
        if current_time % 10 == 0 {
            log_debug("Performing periodic memory deduplication");
            if let Err(e) = memory_broker::deduplicate_memories(mcp_host.as_ref().unwrap()).await {
                log_error(&format!("Failed to deduplicate memories: {}", e));
            }
        }
        
        // Then retrieve all memories
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
        if config.enable_memory_broker.unwrap_or(true) && is_memory_tool_query {
            log_debug("Memory broker is enabled but skipped because the query appears to directly target memory tools");
        }
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
            
            // Check if any function calls contain memory-related operations
            let using_memory_tools = function_calls.iter()
                .any(|call| call.name.to_lowercase().contains("memory"));
            
            // Store memory from response if auto memory is enabled
            if config.enable_auto_memory.unwrap_or(true) && mcp_host.is_some() && !using_memory_tools {
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
            } else if config.enable_auto_memory.unwrap_or(true) && using_memory_tools {
                log_debug("Auto memory extraction skipped because the model is directly using memory tools");
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
                                result.clone(),   // The result from function execution
                                Some(&chat_history), // Pass the chat history
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
                                    
                                    // Check if this was a memory-related function call
                                    let is_memory_function = function_call.name.to_lowercase().contains("memory");
                                    
                                    // Store memories from the function call response
                                    if config.enable_auto_memory.unwrap_or(true) && mcp_host.is_some() && !is_memory_function {
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
                                    } else if config.enable_auto_memory.unwrap_or(true) && is_memory_function {
                                        log_debug("Auto memory extraction skipped because the model is directly using memory tools");
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
                if config.enable_auto_memory.unwrap_or(true) && mcp_host.is_some() && !using_memory_tools {
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
                } else if config.enable_auto_memory.unwrap_or(true) && using_memory_tools {
                    log_debug("Auto memory extraction skipped because the model is directly using memory tools");
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

/// Runs an interactive chat session with the Gemini model.
pub async fn run_interactive_chat(
    args: &Args,
    config: &AppConfig,
    client: &Client,
    mcp_host: &Option<McpHost>,
    api_key: &str,
    system_prompt: &str,
    config_dir: &Path,
    session_id: &str,
    should_save_history: bool,
) -> Result<(), Box<dyn Error>> {
    println!("{}", "Starting interactive chat mode. Type 'exit' or 'quit' to end the session.".cyan());
    println!("{}", "Press Ctrl+C at any time to exit.".cyan());
    println!();
    
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
    
    // Main chat loop
    loop {
        // Prompt for user input
        print!("{} ", "You:".green().bold());
        io::stdout().flush()?;
        
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        
        // Trim the input
        let input = input.trim();
        
        // Exit conditions
        if input.eq_ignore_ascii_case("exit") || input.eq_ignore_ascii_case("quit") {
            println!("{}", "Exiting interactive mode.".cyan());
            break;
        }
        
        // Skip empty inputs
        if input.is_empty() {
            continue;
        }
        
        // Add user message to history
        let user_message = ChatMessage {
            role: roles::USER.to_string(),
            content: input.to_string(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        chat_history.messages.push(user_message);
        
        // Call API
        let spinner = ProgressBar::new_spinner();
        spinner.set_style(ProgressStyle::default_spinner()
            .tick_strings(&["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"])
            .template("{spinner:.green} {msg}")
            .unwrap());
        spinner.set_message("Asking Gemini...".to_string());
        spinner.enable_steady_tick(Duration::from_millis(80));
        
        match call_gemini_api(
            client,
            api_key,
            Some(&system_prompt_with_history),
            input,
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
                
                // Print the response
                println!("{} ", "Gemini:".blue().bold());
                print_gemini_response(&response, false);
                println!();
                
                // Handle function calls (if any)
                for function_call in &function_calls {
                    if let Some(host) = mcp_host {
                        match process_function_call(function_call, host).await {
                            Ok(result) => {
                                println!("\n{}: {}", "Function result".cyan(), 
                                         serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()));
                                
                                // Store the original function result for history
                                let function_result_str = format!("Function '{}' executed successfully with result: {}", 
                                                            function_call.name, 
                                                            serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()));
                                
                                // Send the function result back to the model to get final answer
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
                                    input, // Original user prompt
                                    function_call,    // The function call made by the model
                                    result.clone(),   // The result from function execution
                                    Some(&chat_history), // Pass the chat history
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
                                        println!("{} ", "Gemini (after function):".blue().bold());
                                        print_gemini_response(&final_response, false);
                                        println!();
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
                
                // Save updated history if enabled
                if should_save_history {
                    save_chat_history(&config_dir, &chat_history)?;
                }
                
                // Check if we need to summarize history
                if should_save_history && estimate_total_tokens(&chat_history, &system_prompt) > TOKEN_THRESHOLD {
                    println!("\n{}", "Chat history is getting long. Summarizing...".cyan());
                    match summarize_conversation(client, api_key, &chat_history).await {
                        Ok(new_history) => {
                            chat_history = new_history;
                            save_chat_history(&config_dir, &chat_history)?;
                            println!("{}", "History summarized successfully.".green());
                        },
                        Err(e) => {
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
    }
    
    Ok(())
}

/// Runs a task loop where the AI works on a specific task until completion or failure.
pub async fn run_task_loop(
    args: &Args,
    config: &AppConfig,
    client: &Client,
    mcp_host: &Option<McpHost>,
    api_key: &str,
    system_prompt: &str,
    config_dir: &Path,
    session_id: &str,
    should_save_history: bool,
    task: &str,
) -> Result<(), Box<dyn Error>> {
    println!("{}", "Starting task loop mode.".cyan());
    println!("{}", "Task: ".cyan().bold().to_string() + task);
    println!("{}", "The AI will work on this task and ask for input when needed.".cyan());
    println!("{}", "Press Ctrl+C at any time to exit.".cyan());
    println!();
    
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
    
    // Create task loop specific system prompt
    let task_system_prompt = format!(
        "You are operating in a task loop mode. Your objective is to complete the specified task.\n\
        - If you need specific information from the user, ask your question clearly and end your response precisely with \"WAITING_FOR_USER_INPUT\".\n\
        - Request tool usage when necessary. Available tools are listed in your context.\n\
        - When the task is fully completed, include \"TASK_COMPLETE: \" followed by a summary in your response. This can be at the beginning or end of your message.\n\
        - If you cannot complete the task, include \"TASK_STUCK: \" followed by the reason in your response. This can be at the beginning or end of your message.\n\
        - Otherwise, provide updates on your progress and continue working autonomously.\n\n\
        {}", system_prompt
    );
    
    // Combine the task system prompt with MCP capabilities information
    let mut full_system_prompt = task_system_prompt;
    if !mcp_capabilities_prompt.is_empty() {
        full_system_prompt.push_str("\n\n");
        full_system_prompt.push_str(&mcp_capabilities_prompt);
    }
    
    // Add task to history as the first user message
    let formatted_task = format!("Start Task: {}", task);
    let user_message = ChatMessage {
        role: roles::USER.to_string(),
        content: formatted_task.clone(),
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    };
    chat_history.messages.push(user_message);
    
    // Main task loop
    let mut task_complete = false;
    
    while !task_complete {
        let spinner = ProgressBar::new_spinner();
        spinner.set_style(ProgressStyle::default_spinner()
            .tick_strings(&["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"])
            .template("{spinner:.green} {msg}")
            .unwrap());
        spinner.set_message("AI working on task...".to_string());
        spinner.enable_steady_tick(Duration::from_millis(80));
        
        // Get the latest user message from history for sending to API
        let latest_user_message = chat_history.messages.iter()
            .rev()
            .find(|msg| msg.role == roles::USER)
            .map(|msg| msg.content.clone())
            .unwrap_or_else(|| formatted_task.clone());
            
        // Call the Gemini API
        match call_gemini_api(
            client,
            api_key,
            Some(&full_system_prompt),
            &latest_user_message,
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
                
                // Check for task complete or stuck signal (anywhere in the response)
                if response.contains("TASK_COMPLETE:") {
                    println!("{} ", "✅ Task Complete:".green().bold());
                    // If the signal is at the end, extract the completion message
                    let completion_message = if let Some(idx) = response.find("TASK_COMPLETE:") {
                        response[idx + "TASK_COMPLETE:".len()..].trim()
                    } else {
                        &response
                    };
                    print_gemini_response(completion_message, false);
                    println!();
                    task_complete = true;
                    continue;
                } else if response.contains("TASK_STUCK:") {
                    println!("{} ", "❌ Task Stuck:".red().bold());
                    // If the signal is at the end, extract the stuck message
                    let stuck_message = if let Some(idx) = response.find("TASK_STUCK:") {
                        response[idx + "TASK_STUCK:".len()..].trim()
                    } else {
                        &response
                    };
                    print_gemini_response(stuck_message, false);
                    println!();
                    task_complete = true;
                    continue;
                }
                
                // Check for waiting for user input
                let needs_user_input = response.trim().contains("WAITING_FOR_USER_INPUT");
                let display_response = if needs_user_input {
                    response.replace("WAITING_FOR_USER_INPUT", "")
                } else {
                    response.clone()
                };
                
                // Print the response
                println!("{} ", "AI:".blue().bold());
                print_gemini_response(&display_response, false);
                println!();
                
                // Handle function calls (if any)
                let mut function_executed = false;
                
                for function_call in &function_calls {
                    function_executed = true;
                    if let Some(host) = mcp_host {
                        println!("{} {}", "📌 Executing function:".yellow().bold(), function_call.name);
                        
                        match process_function_call(function_call, host).await {
                            Ok(result) => {
                                println!("{}: {}", "Function result".cyan(), 
                                         serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()));
                                
                                // Store the function result for history
                                let function_result_str = format!("Function '{}' executed successfully with result: {}", 
                                                            function_call.name, 
                                                            serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()));
                                
                                // Add the function execution result to history as a system message
                                chat_history.messages.push(ChatMessage {
                                    role: roles::SYSTEM.to_string(),
                                    content: function_result_str,
                                    timestamp: SystemTime::now()
                                        .duration_since(UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_secs(),
                                });
                                
                                // No need to process the function result further here - the next iteration will handle it
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
                
                // If we need user input, get it
                if needs_user_input {
                    print!("{} ", "You:".green().bold());
                    io::stdout().flush()?;
                    
                    let mut input = String::new();
                    io::stdin().read_line(&mut input)?;
                    
                    // Trim the input
                    let input = input.trim();
                    
                    // Exit condition
                    if input.eq_ignore_ascii_case("exit") || input.eq_ignore_ascii_case("quit") {
                        println!("{}", "Exiting task loop.".cyan());
                        task_complete = true;
                        continue;
                    }
                    
                    // Add user message to history
                    let user_message = ChatMessage {
                        role: roles::USER.to_string(),
                        content: format!("User Response: {}", input),
                        timestamp: SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                    };
                    chat_history.messages.push(user_message);
                }
                
                // If no function was executed and no user input needed, the model should continue
                // This will be handled in the next loop iteration
                
                // Save updated history if enabled
                if should_save_history {
                    save_chat_history(&config_dir, &chat_history)?;
                }
                
                // Check if we need to summarize history
                if should_save_history && estimate_total_tokens(&chat_history, &system_prompt) > TOKEN_THRESHOLD {
                    println!("\n{}", "Chat history is getting long. Summarizing...".cyan());
                    match summarize_conversation(client, api_key, &chat_history).await {
                        Ok(new_history) => {
                            chat_history = new_history;
                            save_chat_history(&config_dir, &chat_history)?;
                            println!("{}", "History summarized successfully.".green());
                        },
                        Err(e) => {
                            eprintln!("{}: {}", "Failed to summarize history".red(), e);
                        }
                    }
                }
                
                // If we executed a function or got user input, continue to the next iteration
                // Otherwise, introduce a small delay to avoid rapid API calls if the model is just giving updates
                if !function_executed && !needs_user_input {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            },
            Err(e) => {
                spinner.finish_and_clear();
                eprintln!("{}: {}", "Error calling Gemini API".red(), e);
                
                // After an error, wait a bit before retrying
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
    
    Ok(())
}