use anyhow::{anyhow, Context, Result};
use dirs::home_dir;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

// Constants for default configuration content
const DEFAULT_MCP_SERVERS_CONFIG: &str = r#"{
  "servers": []
}"#;

const DEFAULT_GEMINI_CONFIG: &str = r#"# Gemini CLI Configuration

[api]
key = ""
model = "gemini-1.5-pro"

[system]
prompt = ""

[features]
history = true
memory = true
"#;

// Get path to a specific configuration file
fn get_config_path(component: &str) -> Result<PathBuf> {
    // First try using standard config directory
    let config_dir = dirs::config_dir()
        .ok_or_else(|| anyhow!("Could not determine config directory"))?
        .join("gemini-suite");
    
    // Check if we need to fall back to the old path for backward compatibility
    if !config_dir.exists() {
        let old_config_dir = home_dir()
            .ok_or_else(|| anyhow!("Could not determine home directory"))?
            .join(".config/gemini-cli");
        
        if old_config_dir.exists() {
            tracing::warn!("Using legacy config path: {}", old_config_dir.display());
            fs::create_dir_all(&config_dir)
                .context("Failed to create new config directory")?;
            
            // TODO: Consider migrating files from old location to new in the future
            return get_old_config_path(component);
        }
    }
    
    fs::create_dir_all(&config_dir)
        .context("Failed to create config directory")?;
    
    let path = match component {
        "mcp-servers" => config_dir.join("mcp_servers.json"),
        "cli" | "gemini-cli" => config_dir.join("config.toml"),
        "happe" => config_dir.join("happe.toml"),
        "ida" => config_dir.join("ida.toml"),
        "mcp-hostd" => config_dir.join("mcp-hostd.toml"),
        _ => return Err(anyhow!("Unknown component: {}", component)),
    };
    
    Ok(path)
}

// Get path using the old directory structure (for backward compatibility)
fn get_old_config_path(component: &str) -> Result<PathBuf> {
    let config_dir = home_dir()
        .ok_or_else(|| anyhow!("Could not determine home directory"))?
        .join(".config/gemini-cli");
    
    fs::create_dir_all(&config_dir)
        .context("Failed to create config directory")?;
    
    let path = match component {
        "mcp-servers" => config_dir.join("mcp_servers.json"),
        "cli" | "gemini-cli" => config_dir.join("config.toml"),
        "happe" => config_dir.join("happe.toml"),
        "ida" => config_dir.join("ida.toml"),
        "mcp-hostd" => config_dir.join("mcp-hostd.toml"),
        _ => return Err(anyhow!("Unknown component: {}", component)),
    };
    
    Ok(path)
}

// Get default configuration content for a component
fn get_default_config(component: &str) -> Result<String> {
    match component {
        "mcp-servers" => Ok(DEFAULT_MCP_SERVERS_CONFIG.to_string()),
        "cli" | "gemini-cli" => Ok(DEFAULT_GEMINI_CONFIG.to_string()),
        "happe" => Ok("# HAPPE daemon configuration\n".to_string()),
        "ida" => Ok("# IDA daemon configuration\n".to_string()),
        "mcp-hostd" => Ok("# MCP host daemon configuration\n".to_string()),
        _ => Err(anyhow!("Unknown component: {}", component)),
    }
}

// Show configuration for a component
pub async fn show_config(component: &str) -> Result<String> {
    let path = get_config_path(component)?;
    
    if !path.exists() {
        return Err(anyhow!("Configuration file for {} does not exist", component));
    }
    
    let content = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read configuration file: {}", path.display()))?;
    
    Ok(content)
}

// Edit configuration for a component
pub async fn edit_config(component: &str) -> Result<()> {
    let path = get_config_path(component)?;
    
    // Create file with default content if it doesn't exist
    if !path.exists() {
        let default_content = get_default_config(component)?;
        fs::write(&path, default_content)
            .with_context(|| format!("Failed to create default configuration file: {}", path.display()))?;
        
        tracing::info!("Created default configuration file: {}", path.display());
    }
    
    // Determine editor to use
    let editor = std::env::var("EDITOR")
        .unwrap_or_else(|_| "nano".to_string());
    
    // Open the file in the editor
    let status = Command::new(&editor)
        .arg(&path)
        .status()
        .with_context(|| format!("Failed to open editor {} for file {}", editor, path.display()))?;
    
    if !status.success() {
        return Err(anyhow!("Editor exited with non-zero status"));
    }
    
    Ok(())
}

// Reset configuration for a component to defaults
pub async fn reset_config(component: &str) -> Result<()> {
    let path = get_config_path(component)?;
    
    if path.exists() {
        // Create backup
        let backup_path = path.with_extension(format!(
            "{}.bak",
            path.extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
        ));
        
        fs::copy(&path, &backup_path)
            .with_context(|| format!("Failed to create backup of configuration file: {}", path.display()))?;
        
        tracing::info!("Created backup at: {}", backup_path.display());
    }
    
    // Write default configuration
    let default_content = get_default_config(component)?;
    fs::write(&path, default_content)
        .with_context(|| format!("Failed to write default configuration to {}", path.display()))?;
    
    Ok(())
} 