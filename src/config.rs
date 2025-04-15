use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::error::Error;
use confy;
use colored::*;

use crate::cli::Args; // Need Args to handle flags
use crate::logging::log_debug; // Use logging

#[derive(Debug, Serialize, Deserialize, Clone)] // Added Clone
pub struct AppConfig {
    pub api_key: Option<String>,
    pub system_prompt: Option<String>,
    pub save_history: Option<bool>,
    pub enable_memory_broker: Option<bool>,
    pub enable_auto_memory: Option<bool>,
    pub memory_broker_model: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            system_prompt: Some(
                "You are a helpful command-line assistant for Linux. \
                You have access to the last few commands the user has run in their terminal. \
                Use this context to provide more relevant answers. When asked about commands, \
                provide concise and practical solutions focused on the user's needs."
                .to_string()
            ),
            save_history: Some(true),
            enable_memory_broker: Some(true),
            enable_auto_memory: Some(true),
            memory_broker_model: Some("gemini-2.0-flash".to_string()),
        }
    }
}

pub fn get_config_dir() -> Result<PathBuf, Box<dyn Error>> {
    confy::get_configuration_file_path("gemini-cli", Some("config.toml"))?
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| "Could not determine config directory".into())
}

pub fn get_config_file_path(config_dir: &Path) -> PathBuf {
    config_dir.join("config.toml")
}

pub fn load_config(path: &Path) -> Result<AppConfig, Box<dyn Error>> {
    match confy::load_path(path) {
        Ok(config) => Ok(config),
        Err(e) => {
            log_debug(&format!("Failed to load config from {}: {}. Using default.", path.display(), e));
            Ok(AppConfig::default())
        }
    }
}

pub fn save_config(path: &Path, cfg: &AppConfig) -> Result<(), Box<dyn Error>> {
    confy::store_path(path, cfg)?;
    log_debug(&format!("Configuration saved to: {}", path.display()));
    Ok(())
}

/// Handles configuration-related flags and returns true if the program should exit.
pub fn handle_config_flags(args: &Args, cfg: &mut AppConfig, config_file_path: &Path) -> Result<bool, Box<dyn Error>> {
    let mut config_updated = false;
    let mut history_config_changed = false;
    let mut memory_config_changed = false;

    // Handle history enable/disable first
    if args.enable_history {
        if cfg.save_history != Some(true) {
            cfg.save_history = Some(true);
            println!("{}", "Conversation history enabled.".green());
            history_config_changed = true;
        }
    } else if args.disable_history {
        if cfg.save_history != Some(false) {
            cfg.save_history = Some(false);
            println!("{}", "Conversation history disabled.".yellow());
            history_config_changed = true;
        }
    }

    // Handle memory broker enable/disable
    if args.enable_memory_broker {
        if cfg.enable_memory_broker != Some(true) {
            cfg.enable_memory_broker = Some(true);
            println!("{}", "Memory broker enabled.".green());
            memory_config_changed = true;
        }
    } else if args.disable_memory_broker {
        if cfg.enable_memory_broker != Some(false) {
            cfg.enable_memory_broker = Some(false);
            println!("{}", "Memory broker disabled.".yellow());
            memory_config_changed = true;
        }
    }

    // Handle auto memory enable/disable
    if args.enable_auto_memory {
        if cfg.enable_auto_memory != Some(true) {
            cfg.enable_auto_memory = Some(true);
            println!("{}", "Auto memory enabled.".green());
            memory_config_changed = true;
        }
    } else if args.disable_auto_memory {
        if cfg.enable_auto_memory != Some(false) {
            cfg.enable_auto_memory = Some(false);
            println!("{}", "Auto memory disabled.".yellow());
            memory_config_changed = true;
        }
    }

    if history_config_changed || memory_config_changed {
        save_config(config_file_path, cfg)?;
        // Don't exit yet, allow other flags to be processed
    }

    // Handle API key and system prompt setting
    if let Some(key) = &args.set_api_key {
        if cfg.api_key.as_deref() != Some(key) {
            cfg.api_key = Some(key.clone());
            config_updated = true;
            println!("{}", "API Key updated.".green());
        }
    }

    if let Some(prompt) = &args.set_system_prompt {
        if cfg.system_prompt.as_deref() != Some(prompt) {
            cfg.system_prompt = Some(prompt.clone());
            config_updated = true;
            println!("{}", "System prompt updated.".green());
        }
    }

    // Handle showing config
    if args.show_config {
        println!("{} ({})", "Current Configuration".cyan().bold(), config_file_path.display());
        let api_key_display = cfg.api_key.as_deref().map_or("Not Set".yellow().to_string(), |k| {
            if k.len() > 8 { format!("{}...", &k[..8]).bright_black().to_string() } else { "Set".green().to_string() }
        });
        println!("  {}: {}", "API Key".blue(), api_key_display);
        let system_prompt_display = cfg.system_prompt.as_deref().map_or("Default".yellow().to_string(), |p| {
            if p.len() > 50 { format!("{}...", &p[..50]).italic().to_string() } else { p.italic().to_string() }
        });
        println!("  {}: {}", "System Prompt".blue(), system_prompt_display);
        let save_history_display = if cfg.save_history.unwrap_or(true) { "Enabled".green() } else { "Disabled".yellow() };
        println!("  {}: {}", "Save History".blue(), save_history_display);
        let memory_broker_display = if cfg.enable_memory_broker.unwrap_or(true) { "Enabled".green() } else { "Disabled".yellow() };
        println!("  {}: {}", "Memory Broker".blue(), memory_broker_display);
        let auto_memory_display = if cfg.enable_auto_memory.unwrap_or(true) { "Enabled".green() } else { "Disabled".yellow() };
        println!("  {}: {}", "Auto Memory".blue(), auto_memory_display);
        println!("  {}: {}", "Memory Broker Model".blue(), cfg.memory_broker_model.as_deref().unwrap_or("gemini-2.0-flash").bright_black());
        return Ok(true); // Exit after showing config
    }

    // Return false means continue execution (no exit needed based on config flags)
    // Unless history/memory config was the *only* thing changed, in which case we can exit
    Ok((history_config_changed || memory_config_changed) && !config_updated && !args.show_config)
} 