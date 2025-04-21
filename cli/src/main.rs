use clap::Parser;
use colored::*;
use dotenv::dotenv;
use std::error::Error;

// Modules used by the refactored CLI
mod app;
mod cli;
mod happe_client; // Renamed from ipc_client
mod logging;
mod output;
// Removed: config, history, memory_broker, utils, gemini-core imports, etc.

// Import the simplified Args
use crate::cli::Args;
// Import the Happe client
use crate::happe_client::HappeClient;
use crate::logging::{log_error, log_info};
use crate::output::print_usage_instructions;

/// Main function - Connects to HAPPE daemon and sends queries
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Initialize logger
    // Consider making log level configurable via Args if needed
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // Load environment variables (might still be useful for HAPPE_IPC_PATH)
    dotenv().ok();

    // Parse command-line arguments
    let args = Args::parse();

    // --- Handle Standalone MCP Server Modes (Kept from original) ---
    // Note: These require gemini-mcp features/dependencies if kept.
    // Ensure Cargo.toml reflects this if these modes are intended to work.
    /*
    if args.filesystem_mcp {
        // Placeholder: Add gemini-mcp dependency and uncomment
        // return gemini_mcp::filesystem::run().await;
        unimplemented!("Filesystem MCP server mode requires gemini-mcp crate");
    } else if args.command_mcp {
        // Placeholder: Add gemini-mcp dependency and uncomment
        // return gemini_mcp::command::run().await;
        unimplemented!("Command MCP server mode requires gemini-mcp crate");
    } else if args.memory_store_mcp {
        // Placeholder: Add gemini-mcp and gemini-memory dependencies and uncomment
        // return gemini_mcp::memory_store::run()
        //     .await
        //     .map_err(|e| e.into());
        unimplemented!("Memory Store MCP server mode requires gemini-mcp/memory crates");
    }
    */
    // --- End Standalone MCP Server Modes ---

    // Initialize HappeClient
    let happe_client = match HappeClient::new(args.happe_ipc_path.clone()) {
        Ok(client) => client,
        Err(e) => {
            log_error(&format!("Failed to initialize Happe Client: {}", e));
            eprintln!("{}", format!("Error initializing IPC client: {}", e).red());
            return Err(e.into());
        }
    };

    // Test connection to HAPPE daemon
    if !happe_client.test_connection().await? {
        eprintln!(
            "{}",
            "Could not connect to HAPPE daemon. Please ensure it is running.".red()
        );
        return Ok(()); // Exit gracefully if cannot connect
    }
    log_info("Successfully connected to HAPPE daemon.");

    // Call app logic based on arguments
    if args.interactive {
        if let Err(e) = crate::app::run_interactive_chat(&happe_client).await {
            log_error(&format!("Error in interactive chat: {}", e));
            eprintln!("{}", format!("Interactive chat failed: {}", e).red());
        }
    } else if let Some(prompt) = args.prompt.clone() {
        if let Err(e) = crate::app::run_single_query(prompt, &happe_client).await {
            log_error(&format!("Error processing prompt: {}", e));
            // Error is already printed in run_single_query
        }
    } else {
        // No prompt and not interactive, show usage
        print_usage_instructions();
    }

    Ok(())
}
