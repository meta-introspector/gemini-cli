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

use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use serde_json::Value;
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
    
    // Enhance query with relevant memories
    let enhanced_prompt = enhance_query_with_memories(&formatted_prompt, &api_key, mcp_host).await?;
    
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
                                    if let Some(api_key_str) = &config.api_key {
                                        match store_memory_from_response(&enhanced_prompt, &final_response, mcp_host, api_key_str).await {
                                            Ok(_) => log_debug("Processed function response for memories"),
                                            Err(e) => log_error(&format!("Failed to process function response for memories: {}", e)),
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
                if let Some(api_key_str) = &config.api_key {
                    match store_memory_from_response(&enhanced_prompt, &response, mcp_host, api_key_str).await {
                        Ok(_) => log_debug("Processed conversation for memories"),
                        Err(e) => log_error(&format!("Failed to process conversation for memories: {}", e)),
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

/// Enhance the user query with relevant memories before sending to the model
async fn enhance_query_with_memories(
    query: &str,
    api_key: &str,
    mcp_host: &Option<McpHost>,
) -> Result<String, Box<dyn Error>> {
    // If no MCP host is available, just return the original query
    if mcp_host.is_none() {
        return Ok(query.to_string());
    }
    
    let host = mcp_host.as_ref().unwrap();
    
    // Check if memory server is available
    let capabilities = host.get_all_capabilities().await;
    let has_memory_server = capabilities.tools.iter().any(|tool| tool.name.starts_with("memory/"));
    
    if !has_memory_server {
        return Ok(query.to_string());
    }
    
    // Get relevant memories from the memory MCP server
    let response = host.execute_tool(
        "memory",
        "get_relevant_memories",
        serde_json::json!({
            "query": query,
            "api_key": api_key
        }),
    ).await;
    
    match response {
        Ok(response) => {
            // Check if we got memories
            if let Some(memories) = response.get("memories").and_then(|m| m.as_array()) {
                if memories.is_empty() {
                    log_debug("No relevant memories found for query");
                    return Ok(query.to_string());
                }
                
                // Format the memories to enhance the query
                let count = response.get("count").and_then(|c| c.as_u64()).unwrap_or(0);
                log_debug(&format!("Found {} relevant memories for query", count));
                
                // Format the memories for the model
                let formatted_memories = memories.iter()
                    .filter_map(|m| {
                        let key = m.get("key").and_then(|k| k.as_str())?;
                        let value = m.get("value").and_then(|v| v.as_str())?;
                        Some(format!("- {} = {}", key, value))
                    })
                    .collect::<Vec<String>>()
                    .join("\n");
                
                // Create enhanced query with memory context
                let enhanced_query = format!(
                    "I have some information stored in my memory that might be relevant to your query:\n\nRELEVANT MEMORIES:\n{}\n\nNow, responding to your query: {}",
                    formatted_memories,
                    query
                );
                
                return Ok(enhanced_query);
            }
            
            // If no memories or couldn't parse them, return original query
            Ok(query.to_string())
        },
        Err(e) => {
            log_error(&format!("Error retrieving memories: {}", e));
            Ok(query.to_string())
        }
    }
}

/// Store important information from assistant's response as memories
async fn store_memory_from_response(
    user_query: &str,
    assistant_response: &str,
    mcp_host: &Option<McpHost>,
    api_key: &str,
) -> Result<(), Box<dyn Error>> {
    // If no MCP host is available, just return
    if mcp_host.is_none() {
        return Ok(());
    }
    
    let host = mcp_host.as_ref().unwrap();
    
    // Check if memory server is available
    let capabilities = host.get_all_capabilities().await;
    let has_memory_server = capabilities.tools.iter().any(|tool| tool.name.starts_with("memory/"));
    
    if !has_memory_server {
        return Ok(());
    }
    
    // First, parse the content for potential memory-worthy information
    // We'll ask Gemini-2.0-flash to extract key information
    let client = reqwest::Client::new();
    let url = "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:generateContent";
    
    let extraction_prompt = format!(
        "Extract key pieces of information from this conversation that would be valuable to remember for future interactions.\n        \n        USER QUERY:\n        \"{}\"\n        \n        ASSISTANT RESPONSE:\n        \"{}\"\n        \n        For each piece of key information, return a JSON object with these properties:\n        - key: A brief, descriptive identifier for this information (e.g., 'project_deadline', 'user_preference_theme')\n        - value: The specific information to remember\n        - tags: An array of 1-3 categorical tags (e.g., ['project', 'timeline'], ['preference', 'ui'])\n        \n        Return these as a JSON array of objects. If no key information is found, return an empty array.\n        Only extract truly important/reusable information. Focus on facts, preferences, project details.\n        \n        Return ONLY valid JSON, no other text.",
        user_query,
        assistant_response
    );
    
    let request_body = serde_json::json!({
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
        return Ok(()); // Continue without storing memories
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
                                return Ok(());
                            }
                        }
                    } else {
                        log_error(&format!("Failed to find JSON array end in response: {}", e));
                        return Ok(());
                    }
                } else {
                    log_error(&format!("Failed to find JSON array start in response: {}", e));
                    return Ok(());
                }
            }
        };
        
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
                let _ = host.execute_tool(
                    "memory",
                    "store_memory",
                    serde_json::json!({
                        "key": key,
                        "value": value,
                        "tags": tags
                    }),
                ).await;
                
                log_debug(&format!("Stored memory: {} = {}", key, value));
            }
        }
    }
    
    Ok(())
} 