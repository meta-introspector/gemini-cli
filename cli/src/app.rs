// Imports needed by the moved logic
use crate::cli::Args;
use crate::history::{
    ChatHistory, ChatMessage, TOKEN_THRESHOLD, estimate_total_tokens, load_chat_history, roles,
    save_chat_history, start_new_chat, summarize_conversation,
};
use crate::logging::{log_error, log_info};
use crate::output::print_gemini_response;
use gemini_core::client::GeminiClient;
use gemini_core::config::GeminiConfig;
use gemini_core::types::{
    Content, FunctionDeclaration, GenerateContentRequest, GenerationConfig, Part, Tool,
};
// Use the re-exported items from the crate root
use crate::{build_mcp_system_prompt, sanitize_json_schema};
// Use memory broker
use crate::memory_broker::MemoryBroker;
// Use our custom enhance_prompt function instead of the gemini-memory one
use crate::utils::enhance_prompt;

use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use serde_json::json;
use serde_json::Value;
use gemini_core::rpc_types::ServerCapabilities;
use std::error::Error;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use anyhow::anyhow; // Import only the anyhow macro for error handling in execute_tool

// Define a context struct to hold common session parameters
#[derive(Clone)]
pub struct SessionContext {
    pub config_dir: PathBuf,
    pub session_id: String,
    pub should_save_history: bool,
    pub system_prompt: String,
}

// Update the imports to include the new McpProvider
use crate::McpProvider;

/// Processes the user's prompt and interacts with the Gemini API.
pub async fn process_prompt(
    args: &Args,
    config: &GeminiConfig,
    gemini_client: &GeminiClient,
    mcp_provider: &McpProvider<'_>,
    memory_store: &Option<MemoryBroker>,
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

    let mut mcp_capabilities_prompt = String::new();
    let mut tools: Option<Vec<Tool>> = None;

    // Get MCP capabilities
    let capabilities = get_capabilities(mcp_provider).await;
    if !capabilities.tools.is_empty() || !capabilities.resources.is_empty() {
        log_info(&format!(
            "MCP Capabilities discovered: {} tools, {} resources",
            capabilities.tools.len(),
            capabilities.resources.len()
        ));

        // Format capabilities for the prompt
        mcp_capabilities_prompt = build_mcp_system_prompt(&capabilities.tools, &capabilities.resources);

        // Generate function declarations for tools
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

    // Combine base system prompt with MCP capabilities text
    let combined_system_prompt = format!("{}\n{}", context.system_prompt, mcp_capabilities_prompt);
    let system_prompt_content = Some(Content {
        parts: vec![Part::text(combined_system_prompt)],
        role: Some("system".to_string()),
    });

    // Format user prompt based on flags
    let formatted_prompt = if args.command_help {
        format!("Provide the Linux command for: {}", prompt)
    } else {
        prompt.to_string()
    };

    // ---- Memory Enhancement ----
    let enhanced_prompt = if config.enable_memory_broker.unwrap_or(true) && memory_store.is_some() {
        let store = memory_store.as_ref().unwrap();
        match enhance_prompt(
            &formatted_prompt,
            store,
            5, // Default top_k
            0.7, // Default min_relevance
        ).await {
            Ok(p) => {
                log_info("Prompt enhanced with memory context.");
                p
            }
            Err(e) => {
                log_error(&format!("Failed to enhance prompt with memory: {}", e));
                formatted_prompt // Fallback to original prompt on error
            }
        }
    } else {
        formatted_prompt // Use original prompt if disabled or store unavailable
    };
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
            let assistant_message_content = response_text.clone(); // Store the text part
            let assistant_message_ts = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
            let assistant_message = ChatMessage {
                role: roles::ASSISTANT.to_string(),
                content: assistant_message_content.clone(),
                timestamp: assistant_message_ts,
                // TODO: Potentially store function calls in history too?
            };
            chat_history.messages.push(assistant_message);

            // ---- Basic Auto Memory ----
            if function_calls.is_empty() // Only store if it wasn't a function call response
                && config.enable_auto_memory.unwrap_or(true)
                && memory_store.is_some()
            {
                let broker = memory_store.as_ref().unwrap();
                // Simple storage: Use prompt as key (or part of key), response as value
                // Consider better key generation later (e.g., hash, summary)
                let key = format!("conv_turn_{}", assistant_message_ts);
                let value = format!("User: {}\nAssistant: {}", enhanced_prompt, assistant_message_content);
                let tags = vec!["auto_memory".to_string(), context.session_id.clone()];

                match broker.store().add_memory(&key, &value, tags, Some(context.session_id.clone()), Some("cli".to_string()), None).await {
                    Ok(_) => log_info(&format!("Stored conversation turn with key: {}", key)),
                    Err(e) => log_error(&format!("Failed to auto-store memory: {}", e)),
                }
            }
            // ---- End Basic Auto Memory ----

            if !function_calls.is_empty() {
                // Process function calls
                let _function_responses: Vec<Part> = Vec::new(); // Prefixed and made immutable
                for function_call in &function_calls {
                    // REMOVE: if let Some(host) = mcp_provider {
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
                        // Use the helper function with the provider directly
                        match execute_tool(mcp_provider, server_name, tool_name, function_call.arguments.clone()).await {
                            Ok(result) => {
                                println!(
                                    "{}: {}",
                                    "Result".green(),
                                    serde_json::to_string_pretty(&result)
                                        .unwrap_or_else(|_| result.to_string())
                                );
                            }
                            Err(e) => {
                                log_error(&format!(
                                    "Function call {} failed: {}",
                                    function_call.name, e
                                ));
                            }
                        }
                    } else {
                        log_error(&format!(
                            "Invalid function call name format: {}",
                            function_call.name
                        ));
                    }
                    // REMOVE: } else { ... MCP host not available error ... }
                }

                // Block that sends function responses back to Gemini is removed for now.
                // TODO: Re-implement function response handling if needed.

                // Since we removed the second API call, we need to print the *initial* response
                // if function calls occurred but no further response is generated.
                // We might need to rethink the flow here - should we always print the initial text?
                // For now, let's just print the initial text to avoid showing nothing.
                print_gemini_response(&response_text, args.command_help);

            } else {
                // No function calls, print the initial response directly
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
    mcp_provider: &McpProvider<'_>,
    memory_store: &Option<MemoryBroker>,
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
    let mut mcp_capabilities_prompt = String::new();
    // REMOVE: if let Some(host) = mcp_provider {
    let capabilities = get_capabilities(mcp_provider).await;
    if !capabilities.tools.is_empty() || !capabilities.resources.is_empty() {
        mcp_capabilities_prompt = build_mcp_system_prompt(&capabilities.tools, &capabilities.resources);
        
        if !capabilities.tools.is_empty() {
            let declarations: Vec<FunctionDeclaration> = capabilities.tools.iter().map(|t| {
                let parameters = t.parameters.clone().unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
                FunctionDeclaration {
                    name: t.name.replace("/", "."),
                    description: t.description.clone(),
                    parameters: sanitize_json_schema(parameters),
                }
            }).collect();
            if !declarations.is_empty() {
                tools = Some(vec![Tool { function_declarations: declarations }]);
            }
        }
    }
    // REMOVE: } // End of removed if let
    
    // Combine base system prompt with MCP capabilities text
    let combined_system_prompt = format!("{}\n{}", context.system_prompt, mcp_capabilities_prompt);
    let system_prompt_content = Some(Content {
        parts: vec![Part::text(combined_system_prompt)],
        role: Some("system".to_string()),
    });

    // Define modes for interaction loop
    #[derive(PartialEq)]
    enum PromptMode {
        User,
        AutoContinue,
        Interrupted,
    }

    let mut current_mode = PromptMode::User;
    let mut consecutive_model_turns = 0;
    let mut auto_continue = args.auto_continue;
    let max_consecutive_turns = args.max_consecutive_turns;

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
                
                // --- Auto Memory (Interactive) ---
                if function_calls.is_empty()
                    && _config.enable_auto_memory.unwrap_or(true)
                    && memory_store.is_some()
                {
                    let broker = memory_store.as_ref().unwrap();
                    let assistant_message_ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
                    let key = format!("conv_turn_{}", assistant_message_ts);
                    // Get the last user message from history for context
                    let last_user_input = chat_history.messages.last().map_or("", |m| &m.content);
                    let value = format!("User: {}\nAssistant: {}", last_user_input, response_text);
                    let tags = vec!["auto_memory".to_string(), "interactive".to_string(), context.session_id.clone()];

                    match broker.store().add_memory(&key, &value, tags, Some(context.session_id.clone()), Some("cli_interactive".to_string()), None).await {
                        Ok(_) => log_info(&format!("Stored interactive turn with key: {}", key)),
                        Err(e) => log_error(&format!("Failed to auto-store interactive memory: {}", e)),
                    }
                }
                // --- End Auto Memory --- 
                
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
                    for function_call in &function_calls {
                        // REMOVE: if let Some(host) = mcp_provider {
                        let _result = crate::utils::execute_tool_with_confirmation(
                            mcp_provider, // Pass the provider directly
                            &function_call.name,
                            function_call.arguments.clone(),
                            None // Use default config for now
                        ).await;
                        // REMOVE: } else { ... log warning ... }
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
    mcp_provider: &McpProvider<'_>,
    memory_store: &Option<MemoryBroker>,
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
        mcp_provider,
        memory_store,
        &task_context,
    ).await
}

/// Runs a task loop where the AI works on a specific task until completion or failure.
pub async fn run_task_loop(
    args: &Args,
    _config: &GeminiConfig,
    gemini_client: &GeminiClient,
    mcp_provider: &McpProvider<'_>,
    memory_store: &Option<MemoryBroker>,
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

    // Get MCP capabilities and construct tools
    let mut tools: Option<Vec<Tool>> = None;
    let capabilities = get_capabilities(mcp_provider).await;
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

    // Track progress across iterations
    let mut progress_percentage: Option<u8> = None;

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
                
                // --- Auto Memory (Task Loop) ---
                if function_calls.is_empty() 
                   && _config.enable_auto_memory.unwrap_or(true)
                   && memory_store.is_some()
                {
                    let broker = memory_store.as_ref().unwrap();
                    let assistant_message_ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
                    let key = format!("task_turn_{}", assistant_message_ts);
                     // Get the last user message (which might be model output in task loop) for context
                    let last_input = chat_history.messages.last().map_or("", |m| &m.content);
                    let value = format!("Input: {}\nOutput: {}", last_input, response_text);
                    let tags = vec!["auto_memory".to_string(), "task_loop".to_string(), task.to_string(), context.session_id.clone()];

                    match broker.store().add_memory(&key, &value, tags, Some(context.session_id.clone()), Some("cli_task".to_string()), None).await {
                        Ok(_) => log_info(&format!("Stored task turn with key: {}", key)),
                        Err(e) => log_error(&format!("Failed to auto-store task memory: {}", e)),
                    }
                }
                // --- End Auto Memory --- 
                
                // Check for progress indicator if enabled
                let mut current_progress: Option<u8> = None;
                if args.progress_reporting {
                    // Extract progress percentage from [PROGRESS: X%] format
                    if let Some(start) = response_text.find("[PROGRESS:") {
                        if let Some(end) = response_text[start..].find("]") {
                            let progress_str = &response_text[start + 10..start + end].trim();
                            if let Some(pct_idx) = progress_str.find('%') {
                                if let Ok(pct) = progress_str[..pct_idx].trim().parse::<u8>() {
                                    current_progress = Some(pct);
                                    progress_percentage = current_progress; // Update the outer variable
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
                if let Some(pct) = current_progress {
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
                    for function_call in &function_calls {
                        let _result = crate::utils::execute_tool_with_confirmation(
                            mcp_provider, // Pass provider directly
                            &function_call.name,
                            function_call.arguments.clone(),
                            Some(crate::utils::ToolExecutionConfig {
                                // Use standard confirmation in task mode
                                confirmation_level: crate::utils::ConfirmationLevel::Standard,
                                timeout_seconds: 60, // Longer timeout for tasks
                                ..Default::default()
                            })
                        ).await;
                    }
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
    if let Some(pct) = progress_percentage {
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

// Function to execute a tool via the appropriate provider
async fn execute_tool(
    provider: &McpProvider<'_>, 
    server_name: &str, 
    tool_name: &str, 
    args: Value
) -> Result<Value, Box<dyn Error>> {
    match provider {
        McpProvider::Host(Some(host)) => {
            // Convert String error to anyhow::Error first, then convert to Box<dyn Error>
            host.execute_tool(server_name, tool_name, args).await
                .map_err(|e| anyhow!(e)) // Convert String -> anyhow::Error
                .map_err(Into::into) // Convert anyhow::Error -> Box<dyn Error>
        },
        McpProvider::Client(client) => {
            // Convert anyhow::Error directly to Box<dyn Error>
            client.execute_tool(server_name, tool_name, args).await
                .map_err(Into::into) 
        },
        McpProvider::Host(None) => {
            Err(anyhow!("MCP host is not available").into())
        }
    }
}

// Function to get capabilities from the appropriate provider
async fn get_capabilities(provider: &McpProvider<'_>) -> ServerCapabilities {
    match provider {
        McpProvider::Host(Some(host)) => {
            host.get_all_capabilities().await
        },
        McpProvider::Client(client) => {
            match client.get_all_capabilities().await {
                Ok(caps) => caps,
                Err(e) => {
                    log_error(&format!("Failed to get capabilities from daemon: {}", e));
                    ServerCapabilities::default() // Return empty capabilities on error
                }
            }
        },
        McpProvider::Host(None) => ServerCapabilities::default() // Return empty capabilities if no host
    }
}
