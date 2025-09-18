use anyhow::{Context, Result};
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use std::io::{self, Write};
use std::time::Duration;
use tracing::{debug, error, info};

use crate::happe_client::HappeClient;
use crate::output::print_happe_response;

/// Runs a single query mode, sending one prompt to the HAPPE daemon and displaying the response
pub async fn run_single_query(prompt: String, happe_client: &HappeClient) -> Result<(String, Option<String>)> {
    info!("Running single query: {}", prompt);
    info!("Using session ID: {}", happe_client.session_id());

    // Display a spinner while waiting for response
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
            .template("{spinner} {msg}")
            .unwrap(),
    );
    spinner.set_message("Processing request...");
    spinner.enable_steady_tick(Duration::from_millis(120));

    // Send request to HAPPE daemon
    match happe_client.send_query(prompt).await {
        Ok(response) => {
            spinner.finish_and_clear();

            if let Some(error) = response.error {
                error!("HAPPE error: {}", error);
                println!("Error: {}", error);
                return Ok(("".to_string(), Some(error)));
            }

            // Log session ID if one was returned
            if let Some(session_id) = &response.session_id {
                debug!("Response session ID: {}", session_id);
            }

            print_happe_response(&response.response);
            Ok((response.response, None))
        }
        Err(e) => {
            spinner.finish_and_clear();
            error!("Failed to send query to HAPPE daemon: {}", e);
            return Err(e.context("Failed to send query to HAPPE daemon"));
        }
    }
}

/// Runs an interactive chat session with the HAPPE daemon
pub async fn run_interactive_chat(happe_client: &HappeClient) -> Result<()> {
    println!("Starting interactive chat session with HAPPE daemon.");
    println!("Session ID: {}", happe_client.session_id().blue());
    println!("Type 'exit' or 'quit' to end the session.");
    println!();

    loop {
        // Prompt for user input
        print!("{}: ", "You".green().bold());
        io::stdout().flush().context("Failed to flush stdout")?;

        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .context("Failed to read input")?;

        let input = input.trim();
        if input.is_empty() {
            continue;
        }

        // Check for exit command
        if input.eq_ignore_ascii_case("exit") || input.eq_ignore_ascii_case("quit") {
            println!("Exiting chat session.");
            break;
        }

        // Display a spinner while waiting for response
        let spinner = ProgressBar::new_spinner();
        spinner.set_style(
            ProgressStyle::default_spinner()
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
                .template("{spinner} {msg}")
                .unwrap(),
        );
        spinner.set_message("Processing request...");
        spinner.enable_steady_tick(Duration::from_millis(120));

        // Send request to HAPPE daemon
        debug!("Sending query to HAPPE daemon: {}", input);
        match happe_client.send_query(input.to_string()).await {
            Ok(response) => {
                spinner.finish_and_clear();

                if let Some(error) = response.error {
                    error!("HAPPE error: {}", error);
                    println!("Error: {}", error);
                    continue;
                }

                // Log session ID if one was returned
                if let Some(session_id) = &response.session_id {
                    debug!("Response session ID: {}", session_id);
                    // We don't need to update our session ID as the client will track it
                }

                print_happe_response(&response.response);
            }
            Err(e) => {
                spinner.finish_and_clear();
                error!("Failed to send query to HAPPE daemon: {}", e);
                eprintln!("Error: {}", e);
            }
        }

        println!(); // Add spacing between interactions
    }

    Ok(())
}
