use anyhow::{anyhow, Context, Result};
use gemini_core::config::{get_mcp_servers_config_path, get_unified_config_path, UnifiedConfig};
use std::fs;
use std::process::Command;
use serde::Deserialize;

// Minimal definition for the section needed by the manager
#[allow(dead_code)] // Fields might be used later or are part of a broader pattern
#[derive(Deserialize, Debug)]
struct GeminiApiConfigSection {
    api_key: Option<String>,
}

// This struct is intentionally simplified for the manager
#[allow(dead_code)] // Fields might be used later or are part of a broader pattern
#[derive(Deserialize, Debug)]
struct Config {
    #[serde(rename = "gemini-api")]
    gemini_api: Option<GeminiApiConfigSection>,
}

// Get default configuration content for a component
fn get_default_config(component: &str) -> Result<String> {
    // Create a default unified config
    if component == "unified" {
        let unified_config = UnifiedConfig::default();
        // Serialize to TOML with pretty formatting
        return toml::to_string_pretty(&unified_config)
            .map_err(|e| anyhow!("Failed to serialize default unified config: {}", e));
    }

    // Legacy component-specific defaults (should be phased out)
    match component {
        "mcp-servers" => Ok(r#"{
  "servers": []
}"#
        .to_string()),
        "cli" | "gemini-cli" => Ok(
            "# This file is deprecated. Please use the unified config.toml instead.\n".to_string(),
        ),
        "happe" => Ok(
            "# This file is deprecated. Please use the unified config.toml instead.\n".to_string(),
        ),
        "ida" => Ok(
            "# This file is deprecated. Please use the unified config.toml instead.\n".to_string(),
        ),
        "mcp-hostd" => Ok(
            "# This file is deprecated. Please use the unified config.toml instead.\n".to_string(),
        ),
        _ => Err(anyhow!("Unknown component: {}", component)),
    }
}

// Show configuration for a component
pub async fn show_config(component: &str) -> Result<String> {
    if component == "unified" {
        // Try to load the unified config
        let path = get_unified_config_path()
            .map_err(|e| anyhow!("Could not determine unified config path: {}", e))?;

        if !path.exists() {
            return Err(anyhow!("Unified configuration file does not exist"));
        }

        let content = fs::read_to_string(&path).with_context(|| {
            format!(
                "Failed to read unified configuration file: {}",
                path.display()
            )
        })?;

        return Ok(content);
    }

    // For MCP servers, use the specific config
    if component == "mcp-servers" {
        let path = get_mcp_servers_config_path()
            .map_err(|e| anyhow!("Could not determine MCP servers config path: {}", e))?;

        if !path.exists() {
            return Err(anyhow!("MCP servers configuration file does not exist"));
        }

        let content = fs::read_to_string(&path).with_context(|| {
            format!(
                "Failed to read MCP servers configuration file: {}",
                path.display()
            )
        })?;

        return Ok(content);
    }

    // For component-specific config, try to extract it from the unified config
    let unified_config = UnifiedConfig::load();

    let component_config = match component {
        "cli" | "gemini-cli" => {
            let cli_config = &unified_config.cli;
            toml::to_string_pretty(cli_config)
                .map_err(|e| anyhow!("Failed to serialize CLI config: {}", e))?
        }
        "happe" => {
            let happe_config = &unified_config.happe;
            toml::to_string_pretty(happe_config)
                .map_err(|e| anyhow!("Failed to serialize HAPPE config: {}", e))?
        }
        "ida" => {
            let ida_config = &unified_config.ida;
            toml::to_string_pretty(ida_config)
                .map_err(|e| anyhow!("Failed to serialize IDA config: {}", e))?
        }
        "mcp-hostd" => {
            let mcp_config = &unified_config.mcp;
            toml::to_string_pretty(mcp_config)
                .map_err(|e| anyhow!("Failed to serialize MCP config: {}", e))?
        }
        _ => return Err(anyhow!("Unknown component: {}", component)),
    };

    Ok(component_config)
}

// Edit configuration for a component
pub async fn edit_config(component: &str) -> Result<()> {
    let path = if component == "unified" {
        // Get the unified config path
        let path = get_unified_config_path()
            .map_err(|e| anyhow!("Could not determine unified config path: {}", e))?;

        // Create file with default content if it doesn't exist
        if !path.exists() {
            let default_content = get_default_config(component)?;
            fs::write(&path, default_content).with_context(|| {
                format!(
                    "Failed to create default unified configuration file: {}",
                    path.display()
                )
            })?;

            tracing::info!(
                "Created default unified configuration file: {}",
                path.display()
            );
        }

        path
    } else if component == "mcp-servers" {
        // Get the MCP servers config path
        let path = get_mcp_servers_config_path()
            .map_err(|e| anyhow!("Could not determine MCP servers config path: {}", e))?;

        // Create file with default content if it doesn't exist
        if !path.exists() {
            let default_content = get_default_config(component)?;
            fs::write(&path, default_content).with_context(|| {
                format!(
                    "Failed to create default MCP servers configuration file: {}",
                    path.display()
                )
            })?;

            tracing::info!(
                "Created default MCP servers configuration file: {}",
                path.display()
            );
        }

        path
    } else {
        // For component-specific config, extract it from unified config to a temporary file
        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join(format!("gemini-suite-{}.toml", component));

        // Extract component config from unified config
        let component_config = show_config(component).await?;

        // Write to temp file
        fs::write(&temp_path, component_config).with_context(|| {
            format!(
                "Failed to write component config to temp file: {}",
                temp_path.display()
            )
        })?;

        tracing::info!("Note: Component-specific configs are part of the unified config. Changes to this file will be merged back.");
        temp_path
    };

    // Determine editor to use
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string());

    // Open the file in the editor
    let status = Command::new(&editor).arg(&path).status().with_context(|| {
        format!(
            "Failed to open editor {} for file {}",
            editor,
            path.display()
        )
    })?;

    if !status.success() {
        return Err(anyhow!("Editor exited with non-zero status"));
    }

    // For component-specific configs, read back the edited file and merge changes into unified config
    if component != "unified" && component != "mcp-servers" {
        tracing::info!("Merging {} changes back into unified config", component);

        // Load the current unified config
        let mut unified_config = UnifiedConfig::load();

        // Read edited component config
        let temp_path = std::env::temp_dir().join(format!("gemini-suite-{}.toml", component));
        let edited_content = fs::read_to_string(&temp_path).with_context(|| {
            format!(
                "Failed to read edited component config: {}",
                temp_path.display()
            )
        })?;

        // Parse the component config
        match component {
            "cli" | "gemini-cli" => {
                if let Ok(cli_config) = toml::from_str(&edited_content) {
                    unified_config.cli = cli_config;
                } else {
                    return Err(anyhow!("Failed to parse edited CLI config"));
                }
            }
            "happe" => {
                if let Ok(happe_config) = toml::from_str(&edited_content) {
                    unified_config.happe = happe_config;
                } else {
                    return Err(anyhow!("Failed to parse edited HAPPE config"));
                }
            }
            "ida" => {
                if let Ok(ida_config) = toml::from_str(&edited_content) {
                    unified_config.ida = ida_config;
                } else {
                    return Err(anyhow!("Failed to parse edited IDA config"));
                }
            }
            "mcp-hostd" => {
                if let Ok(mcp_config) = toml::from_str(&edited_content) {
                    unified_config.mcp = mcp_config;
                } else {
                    return Err(anyhow!("Failed to parse edited MCP config"));
                }
            }
            _ => return Err(anyhow!("Unknown component: {}", component)),
        }

        // Save the updated unified config
        unified_config
            .save()
            .map_err(|e| anyhow!("Failed to save updated unified config: {}", e))?;

        // Clean up temp file
        if let Err(e) = fs::remove_file(&temp_path) {
            tracing::warn!(
                "Failed to remove temporary config file {}: {}",
                temp_path.display(),
                e
            );
        }
    }

    Ok(())
}

// Reset configuration for a component to defaults
pub async fn reset_config(component: &str) -> Result<()> {
    if component == "unified" {
        // Get the unified config path
        let path = get_unified_config_path()
            .map_err(|e| anyhow!("Could not determine unified config path: {}", e))?;

        if path.exists() {
            // Create backup
            let backup_path = path.with_extension("toml.bak");

            fs::copy(&path, &backup_path).with_context(|| {
                format!(
                    "Failed to create backup of unified configuration file: {}",
                    path.display()
                )
            })?;

            tracing::info!("Created backup at: {}", backup_path.display());
        }

        // Write default unified configuration
        let default_content = get_default_config(component)?;
        fs::write(&path, default_content).with_context(|| {
            format!(
                "Failed to write default unified configuration to {}",
                path.display()
            )
        })?;

        return Ok(());
    }

    if component == "mcp-servers" {
        // Get the MCP servers config path
        let path = get_mcp_servers_config_path()
            .map_err(|e| anyhow!("Could not determine MCP servers config path: {}", e))?;

        if path.exists() {
            // Create backup
            let backup_path = path.with_extension("json.bak");

            fs::copy(&path, &backup_path).with_context(|| {
                format!(
                    "Failed to create backup of MCP servers configuration file: {}",
                    path.display()
                )
            })?;

            tracing::info!("Created backup at: {}", backup_path.display());
        }

        // Write default MCP servers configuration
        let default_content = get_default_config(component)?;
        fs::write(&path, default_content).with_context(|| {
            format!(
                "Failed to write default MCP servers configuration to {}",
                path.display()
            )
        })?;

        return Ok(());
    }

    // For component-specific configs, reset them in the unified config
    let mut unified_config = UnifiedConfig::load();

    match component {
        "cli" | "gemini-cli" => {
            unified_config.cli = Default::default();
        }
        "happe" => {
            unified_config.happe = Default::default();
        }
        "ida" => {
            unified_config.ida = Default::default();
        }
        "mcp-hostd" => {
            unified_config.mcp = Default::default();
        }
        _ => return Err(anyhow!("Unknown component: {}", component)),
    }

    // Save the updated unified config
    unified_config
        .save()
        .map_err(|e| anyhow!("Failed to save updated unified config: {}", e))?;

    tracing::info!("Reset {} configuration to defaults", component);

    Ok(())
}
