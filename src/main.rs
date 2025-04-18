use clap::Parser;
use colored::*;
use dotenv::dotenv;
use std::env;
use std::error::Error;
use std::fs;

mod app;
mod cli;
mod history;
mod logging;
mod output;
mod utils;
mod mcp;

// Import the new context struct
use crate::app::SessionContext;
// Import from workspace crates
use gemini_core::client::GeminiClient;
use gemini_core::config::{GeminiConfig, get_default_config_dir, get_default_config_file};
use gemini_mcp::{McpHost, load_mcp_servers}; // Assuming load_mcp_servers is pub

use crate::cli::Args;

use crate::history::generate_session_id;
use crate::logging::{log_error, log_info};
use crate::output::print_usage_instructions;

/// Main function - handle command line args and talk to Gemini API
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Initialize logger
    env_logger::init();

    // Load environment variables
    dotenv().ok();

    // Parse command-line arguments
    let args = Args::parse();
    
    // Handle MCP server modes
    if args.filesystem_mcp {
        return crate::mcp::servers::filesystem::run().await;
    } else if args.command_mcp {
        return crate::mcp::servers::command::run().await;
    } else if args.memory_store_mcp {
        return crate::mcp::servers::memory_store::run().await;
    }
    
    // Use gemini_core::config helpers
    let config_dir = get_default_config_dir("gemini-cli")?;
    fs::create_dir_all(&config_dir)?;
    let config_path = get_default_config_file("gemini-cli")?;
    let mut config = GeminiConfig::load_from_file(&config_path)?;

    // --- Inline handle_config_flags logic ---
    let mut config_updated = false;
    if let Some(key) = &args.set_api_key {
        config.api_key = Some(key.clone());
        println!("API key set in config.");
        config_updated = true;
    }
    if let Some(prompt) = &args.set_system_prompt {
        config.system_prompt = Some(prompt.clone());
        println!("System prompt set in config.");
        config_updated = true;
    }
    if let Some(model) = &args.set_model {
        config.model_name = Some(model.clone());
        println!("Model name set in config.");
        config_updated = true;
    }
    // Add other flags as needed...

    if config_updated {
        config.save_to_file(&config_path)?;
        println!("Configuration saved to {}", config_path.display());
        return Ok(()); // Exit after saving config changes
    }
    // --- End of inlined logic ---

    // Validate API key
    let api_key_from_config = config.api_key.clone();
    let api_key_from_env = env::var("GEMINI_API_KEY").ok();

    let api_key = match api_key_from_config {
        Some(key) if !key.is_empty() => Ok(key),
        _ => match api_key_from_env {
            Some(key) if !key.is_empty() => Ok(key),
            _ => Err(
                "API key not found in config or GEMINI_API_KEY env var, or it is empty."
                    .to_string(),
            ),
        },
    };

    let api_key = match api_key {
        Ok(key) => key,
        Err(msg) => {
            eprintln!("{}", msg.red());
            return Ok(()); // Exit gracefully
        }
    };

    // Update config with env var if it was used and config didn't have one
    if config.api_key.is_none() {
        config.api_key = Some(api_key.clone());
    }

    // Create Gemini API client
    let gemini_client = GeminiClient::new(config.clone())?;

    // MCP Host Initialization using gemini_mcp
    let mut mcp_host: Option<McpHost> = None;
    match load_mcp_servers() {
        // Use function from gemini_mcp
        Ok(server_configs) if !server_configs.is_empty() => {
            match McpHost::new(server_configs).await {
                Ok(host) => {
                    if let Ok(system_info) = host.get_system_info().await {
                        log_info(&format!("Resource access test: {}", system_info));
                    }
                    host.log_to_servers("Gemini CLI started", 3).await;
                    mcp_host = Some(host);
                    log_info("MCP server host initialized successfully");
                }
                Err(e) => {
                    log_error(&format!("Failed to create McpHost: {}", e));
                }
            }
        }
        Ok(_) => { /* No servers configured */ }
        Err(e) => {
            log_error(&format!("Failed to load MCP server configs: {}", e));
        }
    }

    // History decision (simplified, assumes bool field in GeminiConfig)
    let should_save_history = config.save_history.unwrap_or(true) && !args.disable_history;

    // System prompt (directly from GeminiConfig)
    let system_prompt = config.system_prompt.clone().unwrap_or_else(|| {
        "You are a helpful assistant that lives in the command line interface. You are friendly, and a professional programmer and developer.".to_string()
    });

    // Session ID generation (remains the same)
    let session_id = generate_session_id();

    // Create SessionContext instance
    let session_context = SessionContext {
        config_dir: config_dir.clone(), // Clone PathBuf
        session_id,
        should_save_history,
        system_prompt,
    };

    // Print session ID export (remains the same)
    if (env::var("DEBUG").is_ok() || session_context.session_id.starts_with("day_")) // Use context
        && !args.disable_history
        && args.prompt.is_some()
    {
        println!(
            "{}",
            "\nTo maintain chat history across commands, run:".cyan()
        );
        println!("export GEMINI_SESSION_ID=\"{}\"", session_context.session_id); // Use context
        println!();
    }

    // Call app logic, passing GeminiClient and SessionContext
    if args.interactive && args.task.is_some() {
        // Handle combined interactive (-i) and task (-t) mode
        if let Err(e) = crate::app::run_interactive_task_chat(
            &args,
            &config,
            &gemini_client,
            &mcp_host,
            &session_context,
            args.task.as_ref().unwrap(),
        )
        .await
        {
            eprintln!("Error in interactive task chat: {}", e);
        }
    } else if args.interactive {
        if let Err(e) = crate::app::run_interactive_chat(
            &args,
            &config,
            &gemini_client,
            &mcp_host,
            &session_context, // Pass context
        )
        .await
        {
            eprintln!("Error in interactive chat: {}", e);
        }
    } else if let Some(task) = &args.task {
        if let Err(e) = crate::app::run_task_loop(
            &args,
            &config,
            &gemini_client,
            &mcp_host,
            &session_context, // Pass context
            task,
        )
        .await
        {
            eprintln!("Error in task loop: {}", e);
        }
    } else if let Some(prompt) = args.prompt.clone() {
        if let Err(e) = crate::app::process_prompt(
            &args,
            &config,
            &gemini_client,
            &mcp_host,
            &session_context, // Pass context
            &prompt,
        )
        .await
        {
            eprintln!("Error processing prompt: {}", e);
        }
    } else {
        print_usage_instructions();
    }

    // Shutdown MCP host (remains the same)
    if let Some(host) = mcp_host {
        host.shutdown().await;
    }

    Ok(())
}
