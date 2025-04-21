use anyhow::{anyhow, Context, Result};
use colored::Colorize;
use dirs::home_dir;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;

// Enum for representing MCP server status
#[derive(Debug, Clone, PartialEq)]
pub enum ServerStatus {
    Enabled,
    Disabled,
}

impl std::fmt::Display for ServerStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServerStatus::Enabled => write!(f, "{}", "Enabled".green()),
            ServerStatus::Disabled => write!(f, "{}", "Disabled".yellow()),
        }
    }
}

// MCP Server configuration - legacy format
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct McpServer {
    pub name: String,
    pub transport: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "command")]
    pub command_string: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "command", default)]
    pub command_array: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub args: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub env: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub auto_execute: Option<Vec<String>>,
}

// Collection of MCP servers - legacy array format
#[derive(Debug, Serialize, Deserialize)]
pub struct McpServers {
    pub servers: Vec<McpServer>,
}

// New Claude-compatible format
#[derive(Debug, Serialize, Deserialize)]
pub struct ClaudeServer {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

// Claude-compatible format with servers as a map
#[derive(Debug, Serialize, Deserialize)]
pub struct ClaudeServersConfig {
    pub mcpServers: HashMap<String, ClaudeServer>,
}

// Get path to MCP servers configuration file
fn get_mcp_config_path() -> Result<PathBuf> {
    // First try the standard config directory
    let config_dir = dirs::config_dir()
        .ok_or_else(|| anyhow!("Could not determine config directory"))?
        .join("gemini-suite");

    // Check if we need to fall back to the old path for backward compatibility
    if !config_dir.exists() {
        let old_config_dir = home_dir()
            .ok_or_else(|| anyhow!("Could not determine home directory"))?
            .join(".config/gemini-cli");

        if old_config_dir.exists() {
            tracing::warn!(
                "Using legacy config path for MCP servers: {}",
                old_config_dir.display()
            );

            let old_path = old_config_dir.join("mcp_servers.json");
            if old_path.exists() {
                return Ok(old_path);
            }
        }
    }

    fs::create_dir_all(&config_dir).context("Failed to create config directory")?;

    Ok(config_dir.join("mcp_servers.json"))
}

// Read MCP servers configuration from any format
fn read_mcp_config() -> Result<McpServers> {
    let config_path = get_mcp_config_path()?;

    match fs::read_to_string(&config_path) {
        Ok(content) => {
            // Try all possible formats

            // 1. Try parsing as ClaudeServersConfig (new format with mcpServers object)
            let claude_format_result: Result<ClaudeServersConfig, _> =
                serde_json::from_str(&content);

            if let Ok(claude_format) = claude_format_result {
                tracing::debug!("Successfully parsed mcp_servers.json as Claude format");
                // Convert to our internal format
                let mut servers = Vec::new();

                for (name, server) in claude_format.mcpServers {
                    servers.push(McpServer {
                        name,
                        transport: "stdio".to_string(), // Claude format assumes stdio
                        connection: None,
                        command_string: Some(server.command),
                        command_array: None,
                        enabled: server.enabled.or(Some(true)), // Default to enabled if not specified
                        args: Some(server.args),
                        env: Some(server.env),
                        auto_execute: Some(Vec::new()), // Claude format doesn't specify auto_execute
                    });
                }

                return Ok(McpServers { servers });
            }

            // 2. Try parsing as McpServers struct with a servers field (gemini format)
            let servers_result: Result<McpServers, _> = serde_json::from_str(&content);

            if let Ok(servers) = servers_result {
                tracing::debug!("Successfully parsed mcp_servers.json as McpServers format");
                return Ok(servers);
            }

            // 3. Try parsing as a Vec<McpServer> (older format)
            let vec_result: Result<Vec<McpServer>, _> = serde_json::from_str(&content);

            match vec_result {
                Ok(servers_vec) => {
                    tracing::debug!(
                        "Successfully parsed mcp_servers.json as Vec<McpServer> format"
                    );
                    // Convert Vec<McpServer> to McpServers
                    Ok(McpServers {
                        servers: servers_vec,
                    })
                }
                Err(e) => {
                    // All parsing attempts failed, return error with original error message
                    Err(anyhow!(
                        "Failed to parse MCP servers config at {}: {}",
                        config_path.display(),
                        e
                    ))
                }
            }
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {
            // If file doesn't exist, return empty config
            Ok(McpServers {
                servers: Vec::new(),
            })
        }
        Err(e) => Err(anyhow!("Failed to read MCP servers config: {}", e)),
    }
}

// Write MCP servers configuration in the Claude-compatible format
fn write_mcp_config(servers: &McpServers) -> Result<()> {
    let config_path = get_mcp_config_path()?;

    // Convert to Claude-compatible format
    let mut claude_servers = HashMap::new();

    for server in &servers.servers {
        let command = server
            .command_string
            .clone()
            .or_else(|| {
                server
                    .command_array
                    .as_ref()
                    .and_then(|cmd| cmd.first().cloned())
            })
            .unwrap_or_default();

        let args = server.args.clone().unwrap_or_else(|| {
            // If we have a command array with more than one element, use all but the first as args
            server
                .command_array
                .as_ref()
                .map(|cmd| {
                    if cmd.len() > 1 {
                        cmd[1..].to_vec()
                    } else {
                        Vec::new()
                    }
                })
                .unwrap_or_default()
        });

        let env = server.env.clone().unwrap_or_default();

        claude_servers.insert(
            server.name.clone(),
            ClaudeServer {
                command,
                args,
                env,
                enabled: server.enabled,
            },
        );
    }

    let claude_config = ClaudeServersConfig {
        mcpServers: claude_servers,
    };

    // Serialize in the Claude-compatible format
    let content = serde_json::to_string_pretty(&claude_config)
        .context("Failed to serialize MCP servers config")?;

    fs::write(&config_path, content).with_context(|| {
        format!(
            "Failed to write MCP servers config to {}",
            config_path.display()
        )
    })?;

    tracing::debug!(
        "Wrote MCP servers config to {} in Claude-compatible format",
        config_path.display()
    );

    Ok(())
}

// List all MCP servers and their status
pub async fn list_servers() -> Result<HashMap<String, ServerStatus>> {
    let config = read_mcp_config()?;
    let mut statuses = HashMap::new();

    // Built-in servers that are always available
    let builtin_servers = ["filesystem", "command", "memory-store"];

    // Add built-in servers to the list
    for server in builtin_servers.iter() {
        statuses.insert(server.to_string(), ServerStatus::Disabled);
    }

    // Check configured servers
    for server in &config.servers {
        let status = if server.enabled.unwrap_or(true) {
            ServerStatus::Enabled
        } else {
            ServerStatus::Disabled
        };

        statuses.insert(server.name.clone(), status);
    }

    Ok(statuses)
}

// Check status of a specific MCP server
pub async fn check_server_status(name: &str) -> Result<ServerStatus> {
    let servers = list_servers().await?;

    servers
        .get(name)
        .cloned()
        .ok_or_else(|| anyhow!("MCP server '{}' not found", name))
}

// Enable an MCP server
pub async fn enable_server(name: &str) -> Result<()> {
    let mut config = read_mcp_config()?;
    let mut found = false;

    // Update server in configuration
    for server in &mut config.servers {
        if server.name == name {
            server.enabled = Some(true);
            found = true;
            break;
        }
    }

    if !found {
        // Check if it's a built-in server
        match name {
            "filesystem" | "command" | "memory-store" => {
                let command_str = get_builtin_server_command(name)?;

                // Create entry for the built-in server
                config.servers.push(McpServer {
                    name: name.to_string(),
                    transport: "stdio".to_string(),
                    command_string: Some(command_str.clone()),
                    command_array: None,
                    connection: None,
                    enabled: Some(true),
                    args: Some(Vec::new()),
                    env: Some(HashMap::new()),
                    auto_execute: Some(vec![]),
                });
            }
            _ => return Err(anyhow!("MCP server '{}' not found", name)),
        }
    }

    // Write updated configuration
    write_mcp_config(&config)?;

    Ok(())
}

// Disable an MCP server
pub async fn disable_server(name: &str) -> Result<()> {
    let mut config = read_mcp_config()?;
    let mut found = false;

    // Update server in configuration
    for server in &mut config.servers {
        if server.name == name {
            server.enabled = Some(false);
            found = true;
            break;
        }
    }

    if !found {
        // Check if it's a built-in server
        match name {
            "filesystem" | "command" | "memory-store" => {
                let command_str = get_builtin_server_command(name)?;

                // Create disabled entry for the built-in server
                config.servers.push(McpServer {
                    name: name.to_string(),
                    transport: "stdio".to_string(),
                    command_string: Some(command_str.clone()),
                    command_array: None,
                    connection: None,
                    enabled: Some(false),
                    args: Some(Vec::new()),
                    env: Some(HashMap::new()),
                    auto_execute: Some(vec![]),
                });
            }
            _ => return Err(anyhow!("MCP server '{}' not found", name)),
        }
    }

    // Write updated configuration
    write_mcp_config(&config)?;

    Ok(())
}

// Get the command to run a built-in server
// Return as a single string for backward compatibility
fn get_builtin_server_command(name: &str) -> Result<String> {
    // Try to find the gemini-cli binary
    let cli_binary = which::which("gemini-cli-bin")
        .or_else(|_| which::which("gemini-cli"))
        .or_else(|_| {
            // Check standard installation locations
            let paths = [
                // ~/.local/bin is a common user installation location
                dirs::home_dir().map(|p| p.join(".local/bin/gemini-cli-bin")),
                dirs::home_dir().map(|p| p.join(".local/bin/gemini-cli")),
                // System-wide locations
                Some(PathBuf::from("/usr/local/bin/gemini-cli-bin")),
                Some(PathBuf::from("/usr/local/bin/gemini-cli")),
                Some(PathBuf::from("/usr/bin/gemini-cli-bin")),
                Some(PathBuf::from("/usr/bin/gemini-cli")),
            ];

            for maybe_path in paths.iter().flatten() {
                if maybe_path.exists() {
                    return Ok(maybe_path.clone());
                }
            }

            // As a last resort, check in cargo target directory
            if let Ok(current_dir) = std::env::current_dir() {
                if let Some(workspace_root) = current_dir
                    .ancestors()
                    .find(|p| p.join("Cargo.toml").exists())
                {
                    let debug_path = workspace_root.join("target/debug/gemini-cli-bin");
                    if debug_path.exists() {
                        return Ok(debug_path);
                    }

                    let release_path = workspace_root.join("target/release/gemini-cli-bin");
                    if release_path.exists() {
                        return Ok(release_path);
                    }
                }
            }

            Err(anyhow!("Could not find gemini-cli binary"))
        })?;

    // Construct the command based on the server name
    let flag = match name {
        "filesystem" => "--filesystem-mcp",
        "command" => "--command-mcp",
        "memory-store" => "--memory-store-mcp",
        _ => return Err(anyhow!("Unknown built-in server: {}", name)),
    };

    Ok(format!("{} {}", cli_binary.display(), flag))
}

// Install a new MCP server
pub async fn install_server(path: &str, custom_name: Option<String>) -> Result<String> {
    let path = PathBuf::from(path);

    // Ensure path exists
    if !path.exists() {
        return Err(anyhow!("Path does not exist: {}", path.display()));
    }

    // Determine server name
    let name = match custom_name {
        Some(name) => name,
        None => path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow!("Could not determine server name from path"))?
            .to_string(),
    };

    // Read existing configuration
    let mut config = read_mcp_config()?;

    // Check if server with this name already exists
    if config.servers.iter().any(|s| s.name == name) {
        return Err(anyhow!("MCP server with name '{}' already exists", name));
    }

    // If path is a directory, assume it's a Python server like the embedding server
    let (command, args) = if path.is_dir() {
        // Look for a server.py file
        let server_py = path.join("server.py");
        if !server_py.exists() {
            return Err(anyhow!(
                "Could not find server.py in directory: {}",
                path.display()
            ));
        }

        // Use python to run the server
        ("python".to_string(), vec![server_py.display().to_string()])
    } else if path.is_file() {
        // Check if the file is executable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = fs::metadata(&path)?;
            let permissions = metadata.permissions();

            if permissions.mode() & 0o111 == 0 {
                return Err(anyhow!("File is not executable: {}", path.display()));
            }
        }

        // Use the file directly as a command
        (path.display().to_string(), vec![])
    } else {
        return Err(anyhow!(
            "Path is not a file or directory: {}",
            path.display()
        ));
    };

    // Add server to configuration using the Claude-compatible format
    config.servers.push(McpServer {
        name: name.clone(),
        transport: "stdio".to_string(),
        command_string: Some(command),
        connection: None,
        enabled: Some(true),
        command_array: None,
        args: Some(args),
        env: Some(HashMap::new()),
        auto_execute: Some(Vec::new()),
    });

    // Write updated configuration
    write_mcp_config(&config)?;

    Ok(name)
}

// Uninstall an MCP server
pub async fn uninstall_server(name: &str) -> Result<()> {
    // Check if it's a built-in server
    match name {
        "filesystem" | "command" | "memory-store" => {
            return Err(anyhow!("Cannot uninstall built-in server: {}", name));
        }
        _ => {}
    }

    // Read existing configuration
    let mut config = read_mcp_config()?;

    // Find and remove the server
    let initial_len = config.servers.len();
    config.servers.retain(|s| s.name != name);

    if config.servers.len() == initial_len {
        return Err(anyhow!("MCP server '{}' not found", name));
    }

    // Write updated configuration
    write_mcp_config(&config)?;

    Ok(())
}

// Migrate MCP servers configuration to the Claude-compatible format
pub async fn migrate_mcp_config() -> Result<()> {
    // Check if we have a configuration file
    let config_path = get_mcp_config_path()?;

    if !config_path.exists() {
        tracing::info!("No MCP configuration found to migrate");
        return Ok(());
    }

    // Read the configuration
    let content = fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read MCP config file: {}", config_path.display()))?;

    if content.trim().is_empty() {
        tracing::info!("MCP configuration is empty, nothing to migrate");
        return Ok(());
    }

    // Try to parse the configuration in any format
    let config = read_mcp_config()?;

    // Create the new config directory
    let new_config_dir = dirs::config_dir()
        .ok_or_else(|| anyhow!("Could not determine config directory"))?
        .join("gemini-suite");

    fs::create_dir_all(&new_config_dir).context("Failed to create new config directory")?;

    // Write the migrated configuration in the Claude-compatible format
    let new_config_path = new_config_dir.join("mcp_servers.json");

    // Use the write_mcp_config function which now writes in Claude-compatible format
    write_mcp_config(&config)?;

    tracing::info!(
        "Successfully migrated MCP configuration to Claude-compatible format at {}",
        new_config_path.display()
    );

    // Create a backup of the old configuration if it's in the old path
    let expected_old_dir = home_dir().unwrap_or_default().join(".config/gemini-cli");
    if config_path.starts_with(expected_old_dir) {
        let backup_path = config_path.with_extension("json.bak");
        fs::copy(&config_path, &backup_path).with_context(|| {
            format!(
                "Failed to create backup of old MCP config at {}",
                backup_path.display()
            )
        })?;

        tracing::info!(
            "Created backup of old configuration at {}",
            backup_path.display()
        );
    }

    Ok(())
}
