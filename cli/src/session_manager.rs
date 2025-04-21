use anyhow::{Context, Result};
use colored::*;
use std::io::{self, Write};
use std::env;

use crate::happe_client::HappeClient;

/// Handles session management operations for the CLI
pub struct SessionManager;

impl SessionManager {
    /// Shows the user all active sessions and lets them select one or create a new session
    pub async fn select_session(happe_client: &HappeClient) -> Result<String> {
        println!("Fetching active sessions...");
        
        // Get list of active sessions
        let sessions = happe_client.list_sessions().await
            .context("Failed to fetch active sessions")?;
        
        if sessions.is_empty() {
            println!("No active sessions found. Creating a new session.");
            return Ok(happe_client.session_id().to_string());
        }
        
        println!("\nActive sessions:");
        
        // Display sessions with their terminal info (if available)
        for (i, session_id) in sessions.iter().enumerate() {
            if session_id.starts_with("term_") {
                // Try to parse the terminal identifier
                let parts: Vec<&str> = session_id.split('_').collect();
                if parts.len() >= 3 {
                    let terminal_id = parts[1];
                    let timestamp = parts[2];
                    println!("  {}. {} (Terminal {})", i + 1, session_id.blue(), terminal_id);
                } else {
                    println!("  {}. {}", i + 1, session_id.blue());
                }
            } else {
                println!("  {}. {}", i + 1, session_id.blue());
            }
        }
        
        println!("  {}. {}", sessions.len() + 1, "Create a new session".green());
        println!();
        
        // Prompt for selection
        let selection = loop {
            print!("Select a session (1-{}): ", sessions.len() + 1);
            io::stdout().flush().context("Failed to flush stdout")?;
            
            let mut input = String::new();
            io::stdin().read_line(&mut input).context("Failed to read input")?;
            
            match input.trim().parse::<usize>() {
                Ok(n) if n >= 1 && n <= sessions.len() + 1 => {
                    break n;
                }
                _ => {
                    println!("Invalid selection. Please enter a number between 1 and {}.", sessions.len() + 1);
                }
            }
        };
        
        // Return the selected session ID or create a new one
        if selection <= sessions.len() {
            let selected_id = &sessions[selection - 1];
            println!("Selected session: {}", selected_id.blue());
            
            // Set environment variable for shell wrapper
            env::set_var("GEMINI_SESSION_ID", selected_id);
            
            Ok(selected_id.to_string())
        } else {
            // Create a new session
            let new_id = happe_client.session_id().to_string();
            println!("Creating a new session: {}", new_id.blue());
            
            // Set environment variable for shell wrapper
            env::set_var("GEMINI_SESSION_ID", &new_id);
            
            Ok(new_id)
        }
    }
    
    /// Determines if the current session is running in a new terminal compared to the session ID
    pub fn is_new_terminal(session_id: &str) -> bool {
        // Check if the session ID has the terminal prefix format
        if !session_id.starts_with("term_") {
            return true; // Not a terminal-based session ID
        }
        
        // Parse the parent PID from the session ID
        let parts: Vec<&str> = session_id.split('_').collect();
        if parts.len() < 2 {
            return true; // Malformed session ID
        }
        
        if let Ok(session_ppid) = parts[1].parse::<u32>() {
            // Get current terminal's parent PID (the shell)
            if let Ok(current_ppid) = std::process::Command::new("sh")
                .arg("-c")
                .arg("ps -o ppid= -p $$")
                .output() 
            {
                if let Ok(ppid_str) = String::from_utf8(current_ppid.stdout) {
                    if let Ok(current_parent) = ppid_str.trim().parse::<u32>() {
                        // Compare the parent PIDs
                        return current_parent != session_ppid;
                    }
                }
            }
        }
        
        // If anything fails, assume it's a new terminal
        true
    }
} 