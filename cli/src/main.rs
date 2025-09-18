use clap::Parser;
use colored::*;
use dotenv::dotenv;
use gemini_core::config::UnifiedConfig;
use log::LevelFilter;
use std::error::Error;

// Modules used by the refactored CLI
mod app;
mod cli;
mod happe_client; // Renamed from ipc_client
mod logging;
mod output;
mod session_manager;
// Removed: config, history, memory_broker, utils, gemini-core imports, etc.

// Import the simplified Args
use crate::cli::Args;
// Import the Happe client
use crate::happe_client::HappeClient;
use crate::logging::{log_error, log_info};
use crate::output::print_usage_instructions;
use crate::session_manager::SessionManager;
use std::fs;
use std::io::Write;
use chrono::Local;

/// Main function - Connects to HAPPE daemon and sends queries
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Load unified configuration
    let config = UnifiedConfig::load();

    // Parse command-line arguments early to use `verbose` flag for logging
    let args = Args::parse();

    // Determine log level based on verbose flag and config
    let log_level = if args.verbose {
        LevelFilter::Debug // Or Trace, for maximum verbosity
    } else {
        config
            .cli
            .log_level
            .as_deref()
            .map(|level| match level.to_lowercase().as_str() {
                "trace" => LevelFilter::Trace,
                "debug" => LevelFilter::Debug,
                "info" => LevelFilter::Info,
                "warn" => LevelFilter::Warn,
                "error" => LevelFilter::Error,
                _ => LevelFilter::Info,
            })
            .unwrap_or(LevelFilter::Info)
    };

    // Initialize logger with configured log level
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or(log_level.to_string()),
    )
    .init();

    // Load environment variables (for backward compatibility)
    dotenv().ok();

    // Get HAPPE socket path from args or config
    let happe_ipc_path = args
        .happe_ipc_path
        .or_else(|| config.cli.happe_ipc_path.clone());

    // Initialize HappeClient
    let happe_client = match HappeClient::new(happe_ipc_path) {
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

    // Session selection logic
    if args.new_session {
        // Force a new session by unsetting GEMINI_SESSION_ID
        std::env::remove_var("GEMINI_SESSION_ID");
        log_info("Starting a new session as requested");
    } else if args.select_session {
        // User explicitly wants to select a session
        match SessionManager::select_session(&happe_client).await {
            Ok(session_id) => {
                log_info(&format!("Selected session: {}", session_id));
            }
            Err(e) => {
                log_error(&format!("Error selecting session: {}", e));
                eprintln!("{}", format!("Error selecting session: {}", e).red());
            }
        }
    } else {
        // Check if we're in a different terminal than the current session ID
        let session_id = happe_client.session_id();
        if SessionManager::is_new_terminal(session_id) {
            // We're in a different terminal - check if there are active sessions
            match happe_client.list_sessions().await {
                Ok(sessions) if !sessions.is_empty() => {
                    println!("You have active sessions. Use --select-session to choose one or continue with a new session.");
                }
                _ => {
                    // Either error or no sessions, just continue with the current session
                }
            }
        }
    }

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
    } else if let Some(task_files) = args.task_files {
        let timestamp = Local::now().format("%Y%m%d%H%M%S").to_string();
        let batch_output_dir = format!("batch_run_{}", timestamp);
        fs::create_dir_all(&batch_output_dir)?;
        log_info(&format!("Batch run output directory: {}", batch_output_dir));

        for task_file_path in task_files {
            let task_name = task_file_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown_task");
            let task_output_dir = format!("{}/{}", batch_output_dir, task_name);
            fs::create_dir_all(&task_output_dir)?;

            log_info(&format!("Processing task: {}", task_name));

            let prompt_content = fs::read_to_string(&task_file_path)?;
            fs::write(format!("{}/prompt.txt", task_output_dir), &prompt_content)?;

            match crate::app::run_single_query(prompt_content.clone(), &happe_client).await {
                Ok((response, error)) => {
                    fs::write(format!("{}/response.txt", task_output_dir), response)?;
                    if let Some(err) = error {
                        fs::write(format!("{}/error.txt", task_output_dir), err)?;
                    }
                }
                Err(e) => {
                    let error_message = format!("Failed to execute task {}: {}", task_name, e);
                    log_error(&error_message);
                    fs::write(format!("{}/error.txt", task_output_dir), error_message)?;
                }
            }
        }
        // TODO: Implement Nix derivation generation here, using the batch_output_dir as input.
        // This would involve creating a .nix file that captures the inputs and outputs
        // of this batch run for reproducibility.
    } else {
        // No prompt and not interactive, show usage
        print_usage_instructions();
    }

    Ok(())
}
