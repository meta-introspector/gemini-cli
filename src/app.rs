// Imports needed by the moved logic
use crate::cli::Args;
use crate::history::{
    ChatHistory, ChatMessage, TOKEN_THRESHOLD, estimate_total_tokens, load_chat_history, roles,
    save_chat_history, start_new_chat, summarize_conversation,
};
use crate::logging::{log_error, log_info, log_warning};
use crate::output::print_gemini_response;
use gemini_core::client::GeminiClient;
use gemini_core::config::GeminiConfig;
use gemini_core::types::{
    Content, FunctionDeclaration, GenerateContentRequest, GenerationConfig, Part, Tool,
};
use gemini_mcp::McpHost;

use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use serde_json::{json, Value};
use std::error::Error;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// Define a context struct to hold common session parameters
#[derive(Clone)]
pub struct SessionContext {
    pub config_dir: PathBuf,
    pub session_id: String,
    pub should_save_history: bool,
    pub system_prompt: String,
}

/// Sanitize JSON schema to make it compatible with the Gemini API
/// Removes fields that are not supported by Gemini
fn sanitize_json_schema(mut schema: Value) -> Value {
    // If it's an object, process its fields
    if let Value::Object(ref mut obj) = schema {
        // Remove unsupported fields at this level
        obj.remove("default");
        obj.remove("additionalProperties");
        
        // Process nested properties if any
        if let Some(props_obj) = obj.get_mut("properties") {
            if let Value::Object(props_map) = props_obj {
                for (_, prop_value) in props_map.iter_mut() {
                    // Recursively sanitize each property
                    *prop_value = sanitize_json_schema(prop_value.clone());
                }
            }
        }
        
        // Process items schema for arrays
        if let Some(items) = obj.get_mut("items") {
            *items = sanitize_json_schema(items.clone());
        }
    }
    
    schema
}

/// Processes the user's prompt and interacts with the Gemini API.
pub async fn process_prompt(
    args: &Args,
    _config: &GeminiConfig,
    gemini_client: &GeminiClient,
    mcp_host: &Option<McpHost>,
    context: &SessionContext,
    prompt: &str,
) -> Result<(), Box<dyn Error>> {
    // Start a new chat or use existing history
    let mut chat_history = if context.should_save_history && !args.new_chat {
        load_chat_history(&context.config_dir, &context.session_id)
    } else {
        if args.new_chat && context.should_save_history {
            start_new_chat(&context.config_dir, &context.session_id);
        }
        ChatHistory {
            messages: Vec::new(),
            session_id: context.session_id.clone(),
        }
    };

    let _mcp_capabilities_prompt = String::new();
    let mut tools: Option<Vec<Tool>> = None;

    // Get MCP capabilities if host exists
    if let Some(host) = mcp_host {
        let capabilities = host.get_all_capabilities().await;
        if !capabilities.tools.is_empty() || !capabilities.resources.is_empty() {
            log_info(&format!(
                "MCP Capabilities discovered: {} tools, {} resources",
                capabilities.tools.len(),
                capabilities.resources.len()
            ));

            // Format capabilities for the prompt
            // Assuming build_mcp_system_prompt is now internal or we construct it here
            // For now, just log - system prompt enhancement needs thought
            // mcp_capabilities_prompt = build_mcp_system_prompt(&capabilities.tools, &capabilities.resources);

            // Generate function declarations for tools
            if !capabilities.tools.is_empty() {
                // Assuming generate_gemini_function_declarations is internal or we construct Tool here
                // Let's try constructing the Tool struct directly
                let declarations: Vec<FunctionDeclaration> = capabilities
                    .tools
                    .iter()
                    .map(|t| {
                        // Need sanitize_json_schema logic or assume it's done elsewhere
                        let parameters = t
                            .parameters
                            .clone()
                            .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
                        FunctionDeclaration {
                            name: t.name.replace("/", "."), // Use dot notation for Gemini
                            description: t.description.clone(),
                            parameters: sanitize_json_schema(parameters),
                        }
                    })
                    .collect();
                if !declarations.is_empty() {
                    tools = Some(vec![Tool {
                        function_declarations: declarations,
                    }]);
                }
            }
        }
    }

    // TODO: Re-add mcp_capabilities_prompt to system_prompt if needed
    let system_prompt_content = Some(Content {
        parts: vec![Part::text(context.system_prompt.clone())],
        role: Some("system".to_string()),
    });

    // Format user prompt based on flags
    let formatted_prompt = if args.command_help {
        format!("Provide the Linux command for: {}", prompt)
    } else {
        prompt.to_string()
    };

    // ---- Memory Enhancement Section (Commented Out) ----
    // TODO: Re-implement memory enhancement using MCP tools or direct MemoryStore access
    // let enhanced_prompt = if config.enable_memory_broker.unwrap_or(true) && mcp_host.is_some() { ... } else { formatted_prompt };
    let enhanced_prompt = formatted_prompt; // Use original prompt for now
    // ---- End Memory Enhancement ----

    // Build message history for the API call
    let mut messages_for_api: Vec<Content> = chat_history
        .messages
        .iter()
        .map(|msg| {
            Content {
                parts: vec![Part::text(msg.content.clone())],
                role: Some(msg.role.clone()), // Use role directly from history
            }
        })
        .collect();

    // Add the current user prompt
    messages_for_api.push(Content {
        parts: vec![Part::text(enhanced_prompt.clone())],
        role: Some(roles::USER.to_string()),
    });

    // Add user message to local history object
    let user_message = ChatMessage {
        role: roles::USER.to_string(),
        content: enhanced_prompt.clone(), // Use the (potentially enhanced) prompt
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    };
    chat_history.messages.push(user_message);

    // Call Gemini API
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(&["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"])
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );
    spinner.set_message("Asking Gemini...".to_string());
    spinner.enable_steady_tick(Duration::from_millis(80));

    let request = GenerateContentRequest {
        contents: messages_for_api.clone(),
        system_instruction: system_prompt_content.clone(),
        tools: tools.clone(),
        generation_config: Some(GenerationConfig {
            // Enable response control tokens for more interactive conversations
            temperature: Some(0.8),
            top_p: Some(0.95),
            top_k: Some(40),
            candidate_count: Some(1),
            max_output_tokens: Some(1024),
            response_mime_type: Some("text/plain".to_string()),
        }),
    };

    match gemini_client.generate_content(request).await {
        Ok(response) => {
            let response_text = gemini_client
                .extract_text_from_response(&response)
                .unwrap_or_default();
            let function_calls = gemini_client.extract_function_calls_from_response(&response);

            spinner.finish_and_clear();

            // Add assistant message to history (initial response)
            let assistant_message = ChatMessage {
                role: roles::ASSISTANT.to_string(),
                content: response_text.clone(), // Store the text part
                timestamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                // TODO: Potentially store function calls in history too?
            };
            chat_history.messages.push(assistant_message);

            // ---- Auto Memory Section (Commented Out) ----
            // TODO: Re-implement auto-memory using MCP tools or direct MemoryStore access
            // if config.enable_auto_memory.unwrap_or(true) && mcp_host.is_some() { ... }
            // ---- End Auto Memory ----

            if !function_calls.is_empty() {
                // Process function calls
                let mut function_responses: Vec<Part> = Vec::new();
                for function_call in &function_calls {
                    if let Some(host) = mcp_host {
                        // Assuming process_function_call is now internal or needs rework
                        // Let's directly try to execute the tool via McpHost
                        let qualified_name = &function_call.name.replace(".", "/");
                        let parts: Vec<&str> = qualified_name.splitn(2, "/").collect();
                        if parts.len() == 2 {
                            let server_name = parts[0];
                            let tool_name = parts[1];
                            println!(
                                "{} Calling function {} on server {}",
                                "Action:".blue().bold(),
                                tool_name.cyan(),
                                server_name.cyan()
                            );
                            // TODO: Add confirmation step back if needed (using handle_command_confirmation?)
                            match host
                                .execute_tool(
                                    server_name,
                                    tool_name,
                                    function_call.arguments.clone(),
                                )
                                .await
                            {
                                Ok(result) => {
                                    println!(
                                        "{}: {}",
                                        "Result".green(),
                                        serde_json::to_string_pretty(&result)
                                            .unwrap_or_else(|_| result.to_string())
                                    );
                                    function_responses.push(Part::function_response(
                                        function_call.name.clone(),
                                        result,
                                    ));
                                }
                                Err(e) => {
                                    log_error(&format!(
                                        "Function call {} failed: {}",
                                        function_call.name, e
                                    ));
                                    // Send back an error response to the model
                                    function_responses.push(Part::function_response(
                                        function_call.name.clone(),
                                        json!({ "error": e }),
                                    ));
                                }
                            }
                        } else {
                            log_error(&format!(
                                "Invalid function call name format: {}",
                                function_call.name
                            ));
                            function_responses.push(Part::function_response(
                                function_call.name.clone(),
                                json!({ "error": "Invalid function name format received." }),
                            ));
                        }
                    } else {
                        log_warning(&format!(
                            "Received function call {} but no MCP host is available.",
                            function_call.name
                        ));
                        function_responses.push(Part::function_response(
                            function_call.name.clone(),
                            json!({ "error": "Tool execution environment not available." }),
                        ));
                    }
                }

                // If we got function responses, send them back to the model
                if !function_responses.is_empty() {
                    spinner.set_message("Sending function results...".to_string());
                    spinner.enable_steady_tick(Duration::from_millis(80));

                    let mut messages_for_function_response = messages_for_api; // Use the history up to the user prompt
                    // Add the assistant's first response (containing the function call)
                    messages_for_function_response.push(Content {
                        role: Some(roles::ASSISTANT.to_string()), // Model role
                        parts: vec![Part {
                            text: Some(response_text.clone()).filter(|t| !t.is_empty()),
                            function_call: function_calls.first().cloned(),
                            function_response: None,
                        }],
                    });
                    // Add the function responses from the tool execution
                    messages_for_function_response.push(Content {
                        role: Some(roles::USER.to_string()), // Function role maps to User
                        parts: function_responses,
                    });

                    let function_response_request = GenerateContentRequest {
                        contents: messages_for_function_response,
                        system_instruction: system_prompt_content.clone(),
                        tools: tools.clone(),
                        generation_config: Some(GenerationConfig {
                            // Enable response control tokens
                            temperature: Some(0.8),
                            top_p: Some(0.95),
                            top_k: Some(40),
                            candidate_count: Some(1),
                            max_output_tokens: Some(1024),
                            response_mime_type: Some("text/plain".to_string()),
                        }),
                    };

                    match gemini_client
                        .generate_content(function_response_request)
                        .await
                    {
                        Ok(final_response) => {
                            spinner.finish_and_clear();
                            let final_response_text = gemini_client
                                .extract_text_from_response(&final_response)
                                .unwrap_or_default();
                            // Print the final response
                            print_gemini_response(&final_response_text, args.command_help);

                            // Add final assistant message to history
                            let final_assistant_message = ChatMessage {
                                role: roles::ASSISTANT.to_string(),
                                content: final_response_text,
                                timestamp: SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs(),
                            };
                            chat_history.messages.push(final_assistant_message);

                            // ---- Final Auto Memory Check (Commented Out) ----
                            // TODO: Re-implement auto-memory based on final response
                            // ---- End Final Auto Memory Check ----
                        }
                        Err(e) => {
                            spinner.finish_and_clear();
                            log_error(&format!("Error sending function response: {}", e));
                            eprintln!(
                                "{}",
                                format!("Error getting final response from Gemini: {}", e).red()
                            );
                        }
                    }
                }
            } else {
                // No function calls, just print the initial response
                print_gemini_response(&response_text, args.command_help);
            }

            // Save history if enabled
            if context.should_save_history {
                // Summarization logic (needs refactor to use GeminiClient)
                if estimate_total_tokens(&chat_history, &context.system_prompt) > TOKEN_THRESHOLD {
                    println!(
                        "\n{}",
                        "Chat history is getting long. Summarizing...".cyan()
                    );
                    match summarize_conversation(gemini_client, &chat_history).await {
                        // Updated call signature
                        Ok(new_history) => {
                            chat_history = new_history;
                        }
                        Err(e) => log_error(&format!("Failed to summarize history: {}", e)),
                    }
                }
                if let Err(e) = save_chat_history(&context.config_dir, &chat_history) {
                    log_error(&format!("Failed to save chat history: {}", e));
                }
            }
        }
        Err(e) => {
            spinner.finish_and_clear();
            log_error(&format!("Error calling Gemini API: {}", e));
            eprintln!("{}", format!("Error calling Gemini: {}", e).red());
        }
    }

    Ok(())
}

/// Runs an interactive chat session with model-led interaction support.
/// The model can continue autonomously unless it signals for user input.
pub async fn run_interactive_chat(
    args: &Args,
    _config: &GeminiConfig,
    gemini_client: &GeminiClient,
    mcp_host: &Option<McpHost>,
    context: &SessionContext,
) -> Result<(), Box<dyn Error>> {
    // Show mode status with a colored banner
    println!("{}", "╔══════════════════════════════════════════════════════════════╗".cyan());
    println!("{}", "║              Entering interactive chat mode                  ║".cyan());
    
    if args.auto_continue {
        println!("{}",
            format!("║  [Auto-continue: ON] Model may continue without your input   ║").cyan());
        println!("{}",
            format!("║  Maximum consecutive model turns: {:<2}                        ║", 
                args.max_consecutive_turns).cyan().bold());
    } else {
        println!("{}", "║  [Auto-continue: OFF] Model will wait for your input        ║".cyan());
    }
    
    println!("{}", "║  Type 'exit' or 'quit' to end, '/help' for more commands     ║".cyan());
    println!("{}", "╚══════════════════════════════════════════════════════════════╝".cyan());
    
    let mut chat_history = if context.should_save_history {
        load_chat_history(&context.config_dir, &context.session_id)
    } else {
        ChatHistory {
            messages: Vec::new(),
            session_id: context.session_id.clone(),
        }
    };

    // Set up MCP capabilities and construct tools
    let mut tools: Option<Vec<Tool>> = None;
    if let Some(host) = mcp_host {
        let capabilities = host.get_all_capabilities().await;
        if !capabilities.tools.is_empty() {
            let declarations: Vec<FunctionDeclaration> = capabilities
                .tools
                .iter()
                .map(|t| {
                    let parameters = t
                        .parameters
                        .clone()
                        .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
                    FunctionDeclaration {
                        name: t.name.replace("/", "."),
                        description: t.description.clone(),
                        parameters: sanitize_json_schema(parameters),
                    }
                })
                .collect();
            if !declarations.is_empty() {
                tools = Some(vec![Tool {
                    function_declarations: declarations,
                }]);
            }
        }
    }
    
    // Enhance system prompt to instruct the model about the AWAITING_USER_RESPONSE signal
    // TODO: In the future replace with JSON-based signals for multiple signal types
    let enhanced_system_prompt = if args.progress_reporting {
        format!(
            "{}\n\nYou can continue the conversation proactively. If and only if you specifically need input or a response from the user to proceed, end your message with the exact phrase: AWAITING_USER_RESPONSE\n\nWhen working on tasks, report progress using the format: [PROGRESS: X%] where X is the percentage complete.",
            context.system_prompt
        )
    } else {
        format!(
            "{}\n\nYou can continue the conversation proactively. If and only if you specifically need input or a response from the user to proceed, end your message with the exact phrase: AWAITING_USER_RESPONSE",
            context.system_prompt
        )
    };
    
    let system_prompt_content = Some(Content {
        parts: vec![Part::text(enhanced_system_prompt)],
        role: Some("system".to_string()),
    });

    let mut consecutive_model_turns = 0;
    let max_consecutive_turns = args.max_consecutive_turns; 
    let mut auto_continue = args.auto_continue;
    
    // Track current mode to display appropriate prompt
    #[derive(PartialEq)]
    enum PromptMode {
        User,       // Regular user input
        AutoContinue, // Model is continuing automatically
        Interrupted  // Model was interrupted
    }
    
    let mut current_mode = PromptMode::User;

    loop {
        let mut input = String::new();
        
        match current_mode {
            PromptMode::User => {
                print!("{}> ", "You".green().bold());
                io::stdout().flush()?;
                io::stdin().read_line(&mut input)?;
                input = input.trim().to_string();
                
                // Check for special command to toggle auto-continue
                if input == "/auto" {
                    auto_continue = !auto_continue;
                    println!("{} Auto-continue mode is now {}",
                        "Setting:".yellow().bold(),
                        if auto_continue { "ON".green() } else { "OFF".red() }
                    );
                    continue;
                } else if input == "/interrupt" {
                    println!("{} Model will pause and await input", "Setting:".yellow().bold());
                    consecutive_model_turns = max_consecutive_turns; // Force pause
                    current_mode = PromptMode::Interrupted;
                    continue;
                } else if input == "/help" {
                    println!("\n{}", "Available commands:".yellow().bold());
                    println!("/quit, /exit - Exit the chat");
                    println!("/auto - Toggle auto-continue mode");
                    println!("/interrupt - Interrupt model and await input");
                    println!("/help - Show this help message\n");
                    continue;
                }
            },
            PromptMode::AutoContinue => {
                // Auto-continue with a simple instruction
                input = "Continue.".to_string();
                print!("{}> {}\n", "Auto".blue().bold(), input);
            },
            PromptMode::Interrupted => {
                print!("{}> ", "Interrupted".yellow().bold());
                io::stdout().flush()?;
                io::stdin().read_line(&mut input)?;
                input = input.trim().to_string();
                current_mode = PromptMode::User;
            }
        }

        if input == "exit" || input == "quit" || input == "/exit" || input == "/quit" {
            break;
        }

        if input.is_empty() && current_mode == PromptMode::User {
            continue;
        }

        // Build message history for the API call
        let mut messages_for_api: Vec<Content> = chat_history
            .messages
            .iter()
            .map(|msg| Content {
                parts: vec![Part::text(msg.content.clone())],
                role: Some(msg.role.clone()),
            })
            .collect();
        messages_for_api.push(Content {
            parts: vec![Part::text(input.to_string())],
            role: Some(roles::USER.to_string()),
        });

        // Add user message to local history (if not auto-continued)
        if current_mode != PromptMode::AutoContinue || consecutive_model_turns == 0 {
            let user_message = ChatMessage {
                role: roles::USER.to_string(),
                content: input.to_string(),
                timestamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            };
            chat_history.messages.push(user_message);
        }

        let spinner = ProgressBar::new_spinner();
        spinner.set_style(
            ProgressStyle::default_spinner()
                .tick_strings(&["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"])
                .template("{spinner:.green} {msg}")
                .unwrap(),
        );
        
        spinner.set_message(match current_mode {
            PromptMode::User => "Gemini is thinking...".to_string(),
            PromptMode::AutoContinue => "Gemini is continuing...".to_string(),
            PromptMode::Interrupted => "Gemini is responding...".to_string(),
        });
        
        spinner.enable_steady_tick(Duration::from_millis(80));

        let request = GenerateContentRequest {
            contents: messages_for_api.clone(),
            system_instruction: system_prompt_content.clone(),
            tools: tools.clone(),
            generation_config: Some(GenerationConfig {
                // Enable response control tokens
                temperature: Some(0.8),
                top_p: Some(0.95),
                top_k: Some(40),
                candidate_count: Some(1),
                max_output_tokens: Some(1024),
                response_mime_type: Some("text/plain".to_string()),
            }),
        };

        match gemini_client.generate_content(request).await {
            Ok(response) => {
                let mut response_text = gemini_client
                    .extract_text_from_response(&response)
                    .unwrap_or_default();
                let function_calls = gemini_client.extract_function_calls_from_response(&response);
                spinner.finish_and_clear();
                
                // Check for progress indicator
                let mut progress_percentage: Option<u8> = None;
                if args.progress_reporting {
                    // Extract progress percentage from [PROGRESS: X%] format
                    if let Some(start) = response_text.find("[PROGRESS:") {
                        if let Some(end) = response_text[start..].find("]") {
                            let progress_str = &response_text[start + 10..start + end].trim();
                            if let Some(pct_idx) = progress_str.find('%') {
                                if let Ok(pct) = progress_str[..pct_idx].trim().parse::<u8>() {
                                    progress_percentage = Some(pct);
                                    // Optionally remove the progress indicator from display
                                    let end_pos = start + end + 1;
                                    response_text = format!(
                                        "{}{}",
                                        &response_text[..start],
                                        &response_text[end_pos..]
                                    );
                                }
                            }
                        }
                    }
                }

                // Check for the signal at the end of the response
                let awaiting_user_response = response_text.trim().ends_with("AWAITING_USER_RESPONSE");
                
                // Remove the signal from the displayed text if present
                if awaiting_user_response {
                    response_text = response_text.trim().trim_end_matches("AWAITING_USER_RESPONSE").to_string();
                }

                // Add assistant message to history
                let assistant_message = ChatMessage {
                    role: roles::ASSISTANT.to_string(),
                    content: response_text.clone(),
                    timestamp: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                };
                chat_history.messages.push(assistant_message);

                // Show progress bar if percentage is available
                if let Some(pct) = progress_percentage {
                    let width = 40;
                    let filled = (width as f32 * (pct as f32 / 100.0)) as usize;
                    let empty = width - filled;
                    
                    println!(
                        "{} [{}{}] {}%",
                        "Task Progress:".yellow().bold(),
                        "=".repeat(filled).green(),
                        " ".repeat(empty),
                        pct.to_string().green().bold()
                    );
                }

                // Process function calls if any
                if !function_calls.is_empty() {
                    // Use our new utility for consistent tool execution across modes
                    let mut function_responses: Vec<Part> = Vec::new();
                    
                    for function_call in &function_calls {
                        if let Some(host) = mcp_host {
                            let result = crate::utils::execute_tool_with_confirmation(
                                host,
                                &function_call.name,
                                function_call.arguments.clone(),
                                None // Use default config for now
                            ).await;
                            
                            function_responses.push(
                                crate::utils::tool_result_to_function_response(
                                    &function_call.name, 
                                    result
                                )
                            );
                        } else {
                            log_warning(&format!(
                                "Received function call {} but no MCP host is available.",
                                function_call.name
                            ));
                            function_responses.push(Part::function_response(
                                function_call.name.clone(),
                                json!({ "error": "Tool execution environment not available." }),
                            ));
                        }
                    }

                    if !function_responses.is_empty() {
                        // Send function results back (similar logic)
                        spinner.set_message("Sending function results...".to_string());
                        spinner.enable_steady_tick(Duration::from_millis(80));

                        let mut messages_for_function_response = messages_for_api;
                        messages_for_function_response.push(Content {
                            role: Some(roles::ASSISTANT.to_string()),
                            parts: vec![Part {
                                text: Some(response_text.clone()).filter(|t| !t.is_empty()),
                                function_call: function_calls.first().cloned(),
                                function_response: None,
                            }],
                        });
                        messages_for_function_response.push(Content {
                            role: Some(roles::USER.to_string()),
                            parts: function_responses,
                        });

                        let function_response_request = GenerateContentRequest {
                            contents: messages_for_function_response,
                            system_instruction: system_prompt_content.clone(),
                            tools: tools.clone(),
                            generation_config: Some(GenerationConfig {
                                // Enable response control tokens
                                temperature: Some(0.8),
                                top_p: Some(0.95),
                                top_k: Some(40),
                                candidate_count: Some(1),
                                max_output_tokens: Some(1024),
                                response_mime_type: Some("text/plain".to_string()),
                            }),
                        };

                        match gemini_client
                            .generate_content(function_response_request)
                            .await
                        {
                            Ok(final_response) => {
                                spinner.finish_and_clear();
                                let mut final_response_text = gemini_client
                                    .extract_text_from_response(&final_response)
                                    .unwrap_or_default();
                                
                                // Check for progress in final response
                                let mut final_progress: Option<u8> = None;
                                if args.progress_reporting {
                                    if let Some(start) = final_response_text.find("[PROGRESS:") {
                                        if let Some(end) = final_response_text[start..].find("]") {
                                            let progress_str = &final_response_text[start + 10..start + end].trim();
                                            if let Some(pct_idx) = progress_str.find('%') {
                                                if let Ok(pct) = progress_str[..pct_idx].trim().parse::<u8>() {
                                                    final_progress = Some(pct);
                                                    // Optionally remove the progress indicator from display
                                                    let end_pos = start + end + 1;
                                                    final_response_text = format!(
                                                        "{}{}",
                                                        &final_response_text[..start],
                                                        &final_response_text[end_pos..]
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                                
                                // Check for the signal in the final response
                                let final_awaiting_user_response = 
                                    final_response_text.trim().ends_with("AWAITING_USER_RESPONSE");
                                
                                // Remove the signal from the displayed text if present
                                if final_awaiting_user_response {
                                    final_response_text = final_response_text
                                        .trim()
                                        .trim_end_matches("AWAITING_USER_RESPONSE")
                                        .to_string();
                                }
                                
                                // Display gemini's avatar before response
                                println!("{}:", "Gemini".blue().bold());
                                print_gemini_response(&final_response_text, false);
                                
                                // Show final progress bar if available
                                if let Some(pct) = final_progress {
                                    let width = 40;
                                    let filled = (width as f32 * (pct as f32 / 100.0)) as usize;
                                    let empty = width - filled;
                                    
                                    println!(
                                        "{} [{}{}] {}%",
                                        "Task Progress:".yellow().bold(),
                                        "=".repeat(filled).green(),
                                        " ".repeat(empty),
                                        pct.to_string().green().bold()
                                    );
                                }
                                
                                // Add final assistant message to history
                                let final_assistant_message = ChatMessage {
                                    role: roles::ASSISTANT.to_string(),
                                    content: final_response_text,
                                    timestamp: SystemTime::now()
                                        .duration_since(UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_secs(),
                                };
                                chat_history.messages.push(final_assistant_message);
                                
                                // Update auto-continue based on the final response
                                if auto_continue && !final_awaiting_user_response {
                                    consecutive_model_turns += 1;
                                    if consecutive_model_turns >= max_consecutive_turns {
                                        println!(
                                            "{} Model has taken {} consecutive turns. Awaiting user input.",
                                            "Notice:".yellow().bold(),
                                            max_consecutive_turns
                                        );
                                        current_mode = PromptMode::User;
                                        consecutive_model_turns = 0;
                                    } else {
                                        current_mode = PromptMode::AutoContinue;
                                    }
                                } else {
                                    current_mode = PromptMode::User;
                                    consecutive_model_turns = 0;
                                }
                            }
                            Err(e) => {
                                spinner.finish_and_clear();
                                log_error(&format!("Failed to send function results: {}", e));
                                
                                // Display gemini's avatar before response
                                println!("{}:", "Gemini".blue().bold());
                                print_gemini_response(&response_text, false);
                                
                                current_mode = PromptMode::User; // Require user input on error
                                consecutive_model_turns = 0;
                            }
                        }
                    } else {
                        // No function responses to send
                        // Display gemini's avatar before response
                        println!("{}:", "Gemini".blue().bold());
                        print_gemini_response(&response_text, false);
                        
                        // Update auto-continue based on response signal
                        if auto_continue && !awaiting_user_response {
                            consecutive_model_turns += 1;
                            if consecutive_model_turns >= max_consecutive_turns {
                                println!(
                                    "{} Model has taken {} consecutive turns. Awaiting user input.",
                                    "Notice:".yellow().bold(),
                                    max_consecutive_turns
                                );
                                current_mode = PromptMode::User;
                                consecutive_model_turns = 0;
                            } else {
                                current_mode = PromptMode::AutoContinue;
                            }
                        } else {
                            current_mode = PromptMode::User;
                            consecutive_model_turns = 0;
                        }
                    }
                } else {
                    // No function calls to process
                    // Display gemini's avatar before response
                    println!("{}:", "Gemini".blue().bold());
                    print_gemini_response(&response_text, false);
                    
                    // Update auto-continue based on response signal
                    if auto_continue && !awaiting_user_response {
                        consecutive_model_turns += 1;
                        if consecutive_model_turns >= max_consecutive_turns {
                            println!(
                                "{} Model has taken {} consecutive turns. Awaiting user input.",
                                "Notice:".yellow().bold(),
                                max_consecutive_turns
                            );
                            current_mode = PromptMode::User;
                            consecutive_model_turns = 0;
                        } else {
                            current_mode = PromptMode::AutoContinue;
                        }
                    } else {
                        current_mode = PromptMode::User;
                        consecutive_model_turns = 0;
                    }
                }
            }
            Err(e) => {
                spinner.finish_and_clear();
                log_error(&format!("Failed to generate content: {}", e));
                current_mode = PromptMode::User; // Require user input on error
                consecutive_model_turns = 0;
            }
        }

        // Save chat history after each interaction
        if context.should_save_history {
            // Check if summarization is needed
            if estimate_total_tokens(&chat_history, &context.system_prompt) > TOKEN_THRESHOLD {
                println!(
                    "\n{}",
                    "Chat history is getting long. Summarizing...".cyan()
                );
                match summarize_conversation(gemini_client, &chat_history).await {
                    Ok(new_history) => {
                        chat_history = new_history;
                    }
                    Err(e) => log_error(&format!("Failed to summarize history: {}", e)),
                }
            }
            
            if let Err(e) = save_chat_history(&context.config_dir, &chat_history) {
                log_error(&format!("Failed to save chat history: {}", e));
            }
        }
    }

    Ok(())
}

/// Runs an interactive chat that starts with a specific task, combining interactive mode with task mode.
/// This allows the user to guide the AI's work on a task via interactive prompts.
pub async fn run_interactive_task_chat(
    args: &Args,
    config: &GeminiConfig,
    gemini_client: &GeminiClient,
    mcp_host: &Option<McpHost>,
    context: &SessionContext,
    task: &str,
) -> Result<(), Box<dyn Error>> {
    // Show mode status with a colored banner
    println!("{}", "╔══════════════════════════════════════════════════════════════╗".cyan());
    println!("{}", "║            Entering interactive task mode                    ║".cyan());
    println!("{}",
        format!("║  Task: {:<50}", 
            if task.len() > 50 { format!("{}...", &task[0..47]) } else { task.to_string() })
            .cyan());
    
    if args.auto_continue {
        println!("{}",
            format!("║  [Auto-continue: ON] Model may continue without your input   ║").cyan());
        println!("{}",
            format!("║  Maximum consecutive model turns: {:<2}                        ║", 
                args.max_consecutive_turns).cyan().bold());
    } else {
        println!("{}", "║  [Auto-continue: OFF] Model will wait for your input        ║".cyan());
    }
    
    println!("{}", "║  Type 'exit' or 'quit' to end, '/help' for more commands     ║".cyan());
    println!("{}", "╚══════════════════════════════════════════════════════════════╝".cyan());
    
    // Create a modified context with enhanced system prompt for the task
    let mut task_context = context.clone();
    
    // Enhanced system prompt with progress reporting if enabled
    let task_system_prompt = if args.progress_reporting {
        format!(
            "{}\n\nYour current task is: {}\n\nWork on this task step by step, using available tools as needed. The user can provide guidance at any point. End your message with AWAITING_USER_RESPONSE when you need input.\n\nReport your progress using the format: [PROGRESS: X%] where X is the percentage complete.",
            context.system_prompt,
            task
        )
    } else {
        format!(
            "{}\n\nYour current task is: {}\n\nWork on this task step by step, using available tools as needed. The user can provide guidance at any point. End your message with AWAITING_USER_RESPONSE when you need input.",
            context.system_prompt,
            task
        )
    };
    
    task_context.system_prompt = task_system_prompt;
    
    // Initialize with a task-specific prompt to start the loop
    let mut chat_history = if context.should_save_history {
        load_chat_history(&context.config_dir, &context.session_id)
    } else {
        ChatHistory {
            messages: Vec::new(),
            session_id: context.session_id.clone(),
        }
    };
    
    // Add initial task message to history
    let initial_message = ChatMessage {
        role: roles::USER.to_string(),
        content: "Let's work on the task you've been assigned. First, please analyze what needs to be done and create a step-by-step plan.".to_string(),
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    };
    chat_history.messages.push(initial_message);
    
    // Save the modified history
    if context.should_save_history {
        if let Err(e) = save_chat_history(&context.config_dir, &chat_history) {
            log_error(&format!("Failed to save chat history: {}", e));
        }
    }
    
    // Call the regular interactive chat with the modified context
    run_interactive_chat(
        args,
        config,
        gemini_client,
        mcp_host,
        &task_context,
    ).await
}

/// Runs a task loop where the AI works on a specific task until completion or failure.
pub async fn run_task_loop(
    args: &Args,
    _config: &GeminiConfig,
    gemini_client: &GeminiClient,
    mcp_host: &Option<McpHost>,
    context: &SessionContext,
    task: &str,
) -> Result<(), Box<dyn Error>> {
    // Show mode status with a colored banner
    println!("{}", "╔══════════════════════════════════════════════════════════════╗".cyan());
    println!("{}", "║               Starting autonomous task mode                  ║".cyan());
    println!("{}",
        format!("║  Task: {:<50}", 
            if task.len() > 50 { format!("{}...", &task[0..47]) } else { task.to_string() })
            .cyan());
    
    if args.progress_reporting {
        println!("{}", "║  [Progress Reporting: ON] Model will report task progress    ║".cyan());
    }
    
    println!("{}", "║  Press Ctrl+C to stop the task                              ║".cyan());
    println!("{}", "╚══════════════════════════════════════════════════════════════╝".cyan());
    
    let mut chat_history = if context.should_save_history {
        load_chat_history(&context.config_dir, &context.session_id)
    } else {
        ChatHistory {
            messages: Vec::new(),
            session_id: context.session_id.clone(),
        }
    };

    // TODO: Get MCP capabilities and construct system prompt + tools
    let mut tools: Option<Vec<Tool>> = None;
    if let Some(host) = mcp_host {
        let capabilities = host.get_all_capabilities().await;
        if !capabilities.tools.is_empty() {
            let declarations: Vec<FunctionDeclaration> = capabilities
                .tools
                .iter()
                .map(|t| {
                    let parameters = t
                        .parameters
                        .clone()
                        .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
                    FunctionDeclaration {
                        name: t.name.replace("/", "."),
                        description: t.description.clone(),
                        parameters: sanitize_json_schema(parameters),
                    }
                })
                .collect();
            if !declarations.is_empty() {
                tools = Some(vec![Tool {
                    function_declarations: declarations,
                }]);
            }
        }
    }
    
    // Add task instruction to system prompt with progress reporting if enabled
    let base_system_prompt = if args.progress_reporting {
        format!(
            "{}\n\nYour current task is: {}\n\nYou should work autonomously to complete this task. Use available tools as needed. Report your progress using the format: [PROGRESS: X%] where X is the percentage complete.",
            context.system_prompt,
            task
        )
    } else {
        format!(
            "{}\n\nYour current task is: {}\n\nYou should work autonomously to complete this task. Use available tools as needed. Provide updates on your progress.",
            context.system_prompt,
            task
        )
    };
    
    let full_system_prompt_content = Some(Content {
        parts: vec![Part::text(base_system_prompt)],
        role: Some("system".to_string()),
    });

    // Initial message to kick off the loop
    let initial_message = "Begin working on the task.".to_string();
    let mut latest_user_message = initial_message.clone(); // Start with the initial message

    // Setup Ctrl+C handler
    let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, std::sync::atomic::Ordering::SeqCst);
        println!("\n{}", "Task loop interrupted. Exiting...".yellow().bold());
    })
    .expect("Error setting Ctrl-C handler");

    let mut last_progress: Option<u8> = None;

    while running.load(std::sync::atomic::Ordering::SeqCst) {
        // Build message history for the API call
        let mut messages_for_api: Vec<Content> = chat_history
            .messages
            .iter()
            .map(|msg| Content {
                parts: vec![Part::text(msg.content.clone())],
                role: Some(msg.role.clone()),
            })
            .collect();
        // Add the latest message (either initial or model's response)
        messages_for_api.push(Content {
            parts: vec![Part::text(latest_user_message.clone())],
            role: Some(roles::USER.to_string()), // Treat model response as input for next step
        });

        // Add latest message to history
        let current_message = ChatMessage {
            role: roles::USER.to_string(), // Log it as user input for history consistency
            content: latest_user_message.clone(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        chat_history.messages.push(current_message);

        let spinner = ProgressBar::new_spinner();
        spinner.set_style(
            ProgressStyle::default_spinner()
                .tick_strings(&["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"])
                .template("{spinner:.green} {msg}")
                .unwrap(),
        );
        spinner.set_message("Working on task...".to_string());
        spinner.enable_steady_tick(Duration::from_millis(80));

        let request = GenerateContentRequest {
            contents: messages_for_api.clone(),
            system_instruction: full_system_prompt_content.clone(),
            tools: tools.clone(),
            generation_config: Some(GenerationConfig {
                // Enable response control tokens
                temperature: Some(0.8),
                top_p: Some(0.95),
                top_k: Some(40),
                candidate_count: Some(1),
                max_output_tokens: Some(1024),
                response_mime_type: Some("text/plain".to_string()),
            }),
        };

        // Call the Gemini API
        match gemini_client.generate_content(request).await {
            Ok(response) => {
                let mut response_text = gemini_client
                    .extract_text_from_response(&response)
                    .unwrap_or_default();
                let function_calls = gemini_client.extract_function_calls_from_response(&response);
                spinner.finish_and_clear();
                
                // Check for progress indicator if enabled
                let mut progress_percentage: Option<u8> = None;
                if args.progress_reporting {
                    // Extract progress percentage from [PROGRESS: X%] format
                    if let Some(start) = response_text.find("[PROGRESS:") {
                        if let Some(end) = response_text[start..].find("]") {
                            let progress_str = &response_text[start + 10..start + end].trim();
                            if let Some(pct_idx) = progress_str.find('%') {
                                if let Ok(pct) = progress_str[..pct_idx].trim().parse::<u8>() {
                                    progress_percentage = Some(pct);
                                    last_progress = Some(pct);
                                    // Optionally remove the progress indicator from display
                                    let end_pos = start + end + 1;
                                    response_text = format!(
                                        "{}{}",
                                        &response_text[..start],
                                        &response_text[end_pos..]
                                    );
                                }
                            }
                        }
                    }
                }
                
                // Display model's response
                println!("{}:", "Assistant".blue().bold());
                println!("{}", response_text);
                
                // Show progress bar if percentage is available
                if let Some(pct) = progress_percentage {
                    let width = 40;
                    let filled = (width as f32 * (pct as f32 / 100.0)) as usize;
                    let empty = width - filled;
                    
                    println!(
                        "{} [{}{}] {}%",
                        "Task Progress:".yellow().bold(),
                        "=".repeat(filled).green(),
                        " ".repeat(empty),
                        pct.to_string().green().bold()
                    );
                }

                // Add assistant message to history
                let assistant_message = ChatMessage {
                    role: roles::ASSISTANT.to_string(),
                    content: response_text.clone(),
                    timestamp: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                };
                chat_history.messages.push(assistant_message);

                if !function_calls.is_empty() {
                    // Process function calls using our shared utility
                    let mut function_responses: Vec<Part> = Vec::new();

                    // Add the assistant message containing the function call to the API message list
                    messages_for_api.push(Content {
                        parts: vec![Part {
                            text: Some(response_text.clone()).filter(|t| !t.is_empty()),
                            function_call: function_calls.first().cloned(),
                            function_response: None,
                        }],
                        role: Some(roles::ASSISTANT.to_string()),
                    });

                    for function_call in &function_calls {
                        if let Some(host) = mcp_host {
                            // Use our new utility for consistent tool execution
                            println!("{} Auto-executing tool in task mode", "Task:".blue().bold());
                            
                            let result = crate::utils::execute_tool_with_confirmation(
                                host,
                                &function_call.name,
                                function_call.arguments.clone(),
                                Some(crate::utils::ToolExecutionConfig {
                                    // Use standard confirmation in task mode
                                    confirmation_level: crate::utils::ConfirmationLevel::Standard,
                                    timeout_seconds: 60, // Longer timeout for tasks
                                    ..Default::default()
                                })
                            ).await;
                            
                            function_responses.push(
                                crate::utils::tool_result_to_function_response(
                                    &function_call.name, 
                                    result
                                )
                            );
                        } else {
                            log_warning(&format!(
                                "MCP host not available, cannot execute function: {}",
                                function_call.name
                            ));
                            function_responses.push(Part::function_response(
                                function_call.name.clone(),
                                json!({ "error": "MCP host not available to execute function." }),
                            ));
                        }
                    }

                    // Add function responses to history and prepare for next API call
                    let function_response_message = ChatMessage {
                        role: roles::FUNCTION.to_string(),
                        content: serde_json::to_string(&function_responses).unwrap_or_default(),
                        timestamp: SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                    };
                    chat_history.messages.push(function_response_message);

                    // Add the function response parts to the next API call
                    messages_for_api.push(Content {
                        parts: function_responses,
                        role: Some(roles::FUNCTION.to_string()),
                    });

                    // Set up the next request with the function responses included
                    let function_response_request = GenerateContentRequest {
                        contents: messages_for_api.clone(),
                        system_instruction: full_system_prompt_content.clone(),
                        tools: tools.clone(),
                        generation_config: Some(GenerationConfig {
                            temperature: Some(0.8),
                            top_p: Some(0.95),
                            top_k: Some(40),
                            candidate_count: Some(1),
                            max_output_tokens: Some(1024),
                            response_mime_type: Some("text/plain".to_string()),
                        }),
                    };

                    // Make the follow-up API call to get response after function execution
                    let spinner = ProgressBar::new_spinner();
                    spinner.set_style(
                        ProgressStyle::default_spinner()
                            .tick_strings(&["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"])
                            .template("{spinner:.green} {msg}")
                            .unwrap(),
                    );
                    spinner.set_message("Processing function results...".to_string());
                    spinner.enable_steady_tick(Duration::from_millis(80));

                    match gemini_client.generate_content(function_response_request).await {
                         Ok(final_response) => {
                             spinner.finish_and_clear();
                             let mut final_response_text = gemini_client
                                 .extract_text_from_response(&final_response)
                                 .unwrap_or_default();
                                 
                             // Check for progress indicator in final response
                             let mut final_progress: Option<u8> = None;
                             if args.progress_reporting {
                                 if let Some(start) = final_response_text.find("[PROGRESS:") {
                                     if let Some(end) = final_response_text[start..].find("]") {
                                         let progress_str = &final_response_text[start + 10..start + end].trim();
                                         if let Some(pct_idx) = progress_str.find('%') {
                                             if let Ok(pct) = progress_str[..pct_idx].trim().parse::<u8>() {
                                                 final_progress = Some(pct);
                                                 last_progress = Some(pct);
                                                 // Optionally remove the progress indicator from display
                                                 let end_pos = start + end + 1;
                                                 final_response_text = format!(
                                                     "{}{}",
                                                     &final_response_text[..start],
                                                     &final_response_text[end_pos..]
                                                 );
                                             }
                                         }
                                     }
                                 }
                             }
                             
                             // Display the response with the assistant avatar
                             println!("{}:", "Assistant".blue().bold());
                             println!("{}", final_response_text);
                             
                             // Show progress bar if percentage is available
                             if let Some(pct) = final_progress {
                                 let width = 40;
                                 let filled = (width as f32 * (pct as f32 / 100.0)) as usize;
                                 let empty = width - filled;
                                 
                                 println!(
                                     "{} [{}{}] {}%",
                                     "Task Progress:".yellow().bold(),
                                     "=".repeat(filled).green(),
                                     " ".repeat(empty),
                                     pct.to_string().green().bold()
                                 );
                             }

                             // Add final assistant message to history
                             let final_assistant_message = ChatMessage {
                                 role: roles::ASSISTANT.to_string(),
                                 content: final_response_text.clone(),
                                 timestamp: SystemTime::now()
                                     .duration_since(UNIX_EPOCH)
                                     .unwrap_or_default()
                                     .as_secs(),
                             };
                             chat_history.messages.push(final_assistant_message);

                             // Check if task is complete
                             if final_response_text.contains("Task completed") || 
                                final_response_text.contains("Task Completed") || 
                                (final_progress.is_some() && final_progress.unwrap() >= 100) {
                                 println!("{} Task has been completed!", "Success:".green().bold());
                                 break;
                             }

                             // Use this final response text as input for the next loop iteration
                             latest_user_message = final_response_text;
                         }
                         Err(e) => {
                             spinner.finish_and_clear();
                             log_error(&format!("Error sending function response: {}", e));
                             eprintln!("{}", format!("Error getting final response after function call: {}", e).red());
                             latest_user_message = format!("An error occurred while processing function results: {}. Please try again or continue with the task.", e);
                         }
                    }
                } else {
                     // No function call, use the response text as the next input
                     
                     // Check if task is complete
                     if response_text.contains("Task completed") || 
                        response_text.contains("Task Completed") || 
                        (progress_percentage.is_some() && progress_percentage.unwrap() >= 100) {
                         println!("{} Task has been completed!", "Success:".green().bold());
                         break;
                     }
                     
                     latest_user_message = response_text.clone();
                }
            }
            Err(e) => {
                spinner.finish_and_clear();
                log_error(&format!("Error in task loop API call: {}", e));
                eprintln!("{}", format!("API Error: {}", e).red());
                // Retry with a message about the error
                latest_user_message = format!("An error occurred: {}. Please try again or continue with the task.", e);
                // Wait before retrying
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }

        // Save history if enabled
        if context.should_save_history {
            // Summarization logic
            if estimate_total_tokens(&chat_history, &context.system_prompt) > TOKEN_THRESHOLD {
                println!(
                    "\n{}",
                    "Chat history is getting long. Summarizing...".cyan()
                );
                match summarize_conversation(gemini_client, &chat_history).await {
                    Ok(new_history) => {
                        chat_history = new_history;
                    }
                    Err(e) => log_error(&format!("Failed to summarize history: {}", e)),
                }
            }
            if let Err(e) = save_chat_history(&context.config_dir, &chat_history) {
                log_error(&format!("Failed to save chat history: {}", e));
            }
        }
    }
    
    // Show final progress indicator if available
    if let Some(pct) = last_progress {
        let width = 40;
        let filled = (width as f32 * (pct as f32 / 100.0)) as usize;
        let empty = width - filled;
        
        println!(
            "{} Final progress: [{}{}] {}%",
            "Task Summary:".yellow().bold(),
            "=".repeat(filled).green(),
            " ".repeat(empty),
            pct.to_string().green().bold()
        );
    }

    Ok(())
}
