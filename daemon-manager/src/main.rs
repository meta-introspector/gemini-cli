use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use tracing::{info, debug};
use colored::Colorize;
use std::env;
use std::path::PathBuf;
use dirs;

mod daemon;
mod config;
mod mcp;

// Get the name of the current executable
fn get_executable_name() -> String {
    env::current_exe()
        .ok()
        .and_then(|p| p.file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
        )
        .unwrap_or_else(|| "gemini-manager".to_string())
}

/// Gemini Suite Daemon Manager CLI
/// 
/// A command-line tool for managing gemini-suite daemons and MCP servers
#[derive(Parser, Debug)]
#[command(name = "gemini-manager", author, version, about, long_about = None)]
struct Cli {
    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Daemon management commands
    #[command(subcommand)]
    Daemon(DaemonCommands),

    /// MCP server management commands
    #[command(subcommand)]
    Mcp(McpCommands),

    /// Configuration management commands
    #[command(subcommand)]
    Config(ConfigCommands),

    /// Show status of all daemons and MCP servers
    Status,
    
    /// Start all daemons in the correct order (mcp-hostd -> ida -> happe)
    Start,
    
    /// Stop all daemons in reverse order (happe -> ida -> mcp-hostd)
    Stop,
}

#[derive(Subcommand, Debug)]
enum DaemonCommands {
    /// Start a daemon
    Start {
        /// Name of the daemon to start (happe, ida, mcp-hostd)
        #[arg(required = true)]
        name: String,
    },
    /// Stop a daemon
    Stop {
        /// Name of the daemon to stop (happe, ida, mcp-hostd)
        #[arg(required = true)]
        name: String,
    },
    /// Restart a daemon
    Restart {
        /// Name of the daemon to restart (happe, ida, mcp-hostd)
        #[arg(required = true)]
        name: String,
    },
    /// Check the status of a daemon
    Status {
        /// Name of the daemon to check (happe, ida, mcp-hostd)
        #[arg(required = true)]
        name: String,
    },
    /// Install a daemon to the system
    Install {
        /// Name of the daemon to install (happe, ida, mcp-hostd)
        #[arg(required = true)]
        name: String,
    },
    /// Uninstall a daemon from the system
    Uninstall {
        /// Name of the daemon to uninstall (happe, ida, mcp-hostd)
        #[arg(required = true)]
        name: String,
    },
    /// List all daemons and their status
    List,
}

#[derive(Subcommand, Debug)]
enum McpCommands {
    /// List all MCP servers
    List,
    /// Enable an MCP server
    Enable {
        /// Name of the MCP server to enable
        #[arg(required = true)]
        name: String,
    },
    /// Disable an MCP server
    Disable {
        /// Name of the MCP server to disable
        #[arg(required = true)]
        name: String,
    },
    /// Check status of an MCP server
    Status {
        /// Name of the MCP server to check
        #[arg(required = true)]
        name: String,
    },
    /// Install a new MCP server
    Install {
        /// Path to the server executable or configuration
        #[arg(required = true)]
        path: String,
        
        /// Name for the server (defaults to filename)
        #[arg(short, long)]
        name: Option<String>,
    },
    /// Uninstall an MCP server
    Uninstall {
        /// Name of the MCP server to uninstall
        #[arg(required = true)]
        name: String,
    },
    /// Migrate MCP server configuration to latest format
    Migrate,
}

#[derive(Subcommand, Debug)]
enum ConfigCommands {
    /// Edit configuration for a component
    Edit {
        /// Component to configure (happe, ida, mcp-hostd, mcp-servers)
        #[arg(required = true)]
        component: String,
    },
    /// Show configuration for a component
    Show {
        /// Component to show configuration for (happe, ida, mcp-hostd, mcp-servers)
        #[arg(required = true)]
        component: String,
    },
    /// Reset configuration for a component to defaults
    Reset {
        /// Component to reset configuration for (happe, ida, mcp-hostd, mcp-servers)
        #[arg(required = true)]
        component: String,
    },
}

fn setup_logging(verbose: bool) {
    let level = if verbose {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    };
    
    tracing_subscriber::fmt()
        .with_max_level(level)
        .init();
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    
    setup_logging(cli.verbose);
    
    debug!("Starting gemini-manager with arguments: {:#?}", cli);
    
    match cli.command {
        Commands::Daemon(cmd) => {
            match cmd {
                DaemonCommands::Start { name } => {
                    daemon::start_daemon(&name).await
                        .with_context(|| format!("Failed to start daemon {}", name))?;
                    info!("{} daemon started successfully", name.green());
                }
                DaemonCommands::Stop { name } => {
                    daemon::stop_daemon(&name).await
                        .with_context(|| format!("Failed to stop daemon {}", name))?;
                    info!("{} daemon stopped successfully", name.green());
                }
                DaemonCommands::Restart { name } => {
                    daemon::restart_daemon(&name).await
                        .with_context(|| format!("Failed to restart daemon {}", name))?;
                    info!("{} daemon restarted successfully", name.green());
                }
                DaemonCommands::Status { name } => {
                    let status = daemon::check_daemon_status(&name).await
                        .with_context(|| format!("Failed to check status of daemon {}", name))?;
                    println!("{}: {}", name, status);
                }
                DaemonCommands::Install { name } => {
                    daemon::install_daemon(&name).await
                        .with_context(|| format!("Failed to install daemon {}", name))?;
                    info!("{} daemon installed successfully", name.green());
                }
                DaemonCommands::Uninstall { name } => {
                    daemon::uninstall_daemon(&name).await
                        .with_context(|| format!("Failed to uninstall daemon {}", name))?;
                    info!("{} daemon uninstalled successfully", name.green());
                }
                DaemonCommands::List => {
                    let statuses = daemon::list_daemons().await
                        .context("Failed to list daemons")?;
                    for (name, status) in statuses {
                        println!("{}: {}", name, status);
                    }
                }
            }
        }
        Commands::Mcp(cmd) => {
            match cmd {
                McpCommands::List => {
                    let servers = mcp::list_servers().await
                        .context("Failed to list MCP servers")?;
                    for (name, status) in servers {
                        println!("{}: {}", name, status);
                    }
                }
                McpCommands::Enable { name } => {
                    mcp::enable_server(&name).await
                        .with_context(|| format!("Failed to enable MCP server {}", name))?;
                    info!("MCP server {} enabled successfully", name.green());
                }
                McpCommands::Disable { name } => {
                    mcp::disable_server(&name).await
                        .with_context(|| format!("Failed to disable MCP server {}", name))?;
                    info!("MCP server {} disabled successfully", name.green());
                }
                McpCommands::Status { name } => {
                    let status = mcp::check_server_status(&name).await
                        .with_context(|| format!("Failed to check status of MCP server {}", name))?;
                    println!("{}: {}", name, status);
                }
                McpCommands::Install { path, name } => {
                    let server_name = mcp::install_server(&path, name).await
                        .with_context(|| format!("Failed to install MCP server from {}", path))?;
                    info!("MCP server {} installed successfully", server_name.green());
                }
                McpCommands::Uninstall { name } => {
                    mcp::uninstall_server(&name).await
                        .with_context(|| format!("Failed to uninstall MCP server {}", name))?;
                    info!("MCP server {} uninstalled successfully", name.green());
                }
                McpCommands::Migrate => {
                    info!("Migrating MCP server configuration to Claude-compatible format...");
                    mcp::migrate_mcp_config().await
                        .with_context(|| "Failed to migrate MCP server configuration")?;
                    info!("MCP server configuration successfully migrated to Claude-compatible format.");
                    info!("This format is compatible with Claude Desktop and other MCP clients.");
                }
            }
        }
        Commands::Config(cmd) => {
            match cmd {
                ConfigCommands::Edit { component } => {
                    config::edit_config(&component).await
                        .with_context(|| format!("Failed to edit configuration for {}", component))?;
                    info!("Configuration for {} updated successfully", component.green());
                }
                ConfigCommands::Show { component } => {
                    let config = config::show_config(&component).await
                        .with_context(|| format!("Failed to show configuration for {}", component))?;
                    println!("{}", config);
                }
                ConfigCommands::Reset { component } => {
                    config::reset_config(&component).await
                        .with_context(|| format!("Failed to reset configuration for {}", component))?;
                    info!("Configuration for {} reset to defaults", component.green());
                }
            }
        }
        Commands::Status => {
            println!("{}", "=== Gemini Suite Status ===".bold());
            
            // Show daemon statuses
            println!("\n{}", "Daemons:".bold().underline());
            let daemon_statuses = daemon::list_daemons().await
                .context("Failed to list daemons")?;
            
            let max_name_length = daemon_statuses.keys()
                .map(|name| name.len())
                .max()
                .unwrap_or(10);
            
            // Show daemons in the recommended startup order
            let ordered_daemons = ["mcp-hostd", "ida", "happe"];
            for name in ordered_daemons {
                if let Some(status) = daemon_statuses.get(name) {
                    println!("  {:<width$} : {}", name, status, width = max_name_length);
                }
            }
            
            // Show MCP server statuses
            println!("\n{}", "MCP Servers:".bold().underline());
            let mcp_statuses = mcp::list_servers().await
                .context("Failed to list MCP servers")?;
            
            let max_mcp_name_length = mcp_statuses.keys()
                .map(|name| name.len())
                .max()
                .unwrap_or(10);
            
            // First show built-in servers
            let built_in = ["filesystem", "command", "memory-store"];
            for name in built_in {
                if let Some(status) = mcp_statuses.get(name) {
                    println!("  {:<width$} : {} (built-in)", name, status, width = max_mcp_name_length);
                }
            }
            
            // Then show other servers
            let custom_servers: Vec<_> = mcp_statuses.keys()
                .filter(|name| !built_in.contains(&name.as_str()))
                .collect();
            
            if !custom_servers.is_empty() {
                for name in custom_servers {
                    if let Some(status) = mcp_statuses.get(name) {
                        println!("  {:<width$} : {}", name, status, width = max_mcp_name_length);
                    }
                }
            }
            
            // Check if legacy configuration exists
            let old_config_dir = dirs::home_dir()
                .ok_or_else(|| anyhow!("Could not determine home directory"))?
                .join(".config/gemini-cli");
            
            let old_config_path = old_config_dir.join("mcp_servers.json");
            let new_config_dir = dirs::config_dir()
                .ok_or_else(|| anyhow!("Could not determine config directory"))?
                .join("gemini-suite");
            let new_config_path = new_config_dir.join("mcp_servers.json");
            
            if old_config_path.exists() && !new_config_path.exists() {
                println!("\n{}", "Configuration:".bold().underline());
                println!("  {}: {}", "Legacy MCP config detected".yellow(), "Run 'gemini-manager mcp migrate' to update".blue());
            } else if new_config_path.exists() {
                // Check if it's using the Claude-compatible format
                if let Ok(content) = std::fs::read_to_string(&new_config_path) {
                    if content.contains("\"mcpServers\"") {
                        println!("\n{}", "Configuration:".bold().underline());
                        println!("  {}: {}", "Using Claude-compatible MCP config format".green(), "✓");
                    } else {
                        println!("\n{}", "Configuration:".bold().underline());
                        println!("  {}: {}", "Using legacy MCP config format".yellow(), "Run 'gemini-manager mcp migrate' to update to Claude-compatible format".blue());
                    }
                }
            }
            
            // Show management tips
            println!("\n{}", "Management:".bold().underline());
            println!("  Start all    : {} start", get_executable_name().green());
            println!("  Stop all     : {} stop", get_executable_name().yellow());
            println!("  Configure    : {} config edit <component>", get_executable_name().blue());
        }
        Commands::Start => {
            println!("{}", "Starting all daemons in order...".bold());
            
            // Start mcp-hostd first
            info!("Starting {}...", "mcp-hostd".green());
            daemon::start_daemon("mcp-hostd").await
                .context("Failed to start mcp-hostd daemon")?;
            
            // Wait a moment for mcp-hostd to initialize
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            
            // Start ida next
            info!("Starting {}...", "ida".green());
            daemon::start_daemon("ida").await
                .context("Failed to start ida daemon")?;
            
            // Wait a moment for ida to initialize
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            
            // Start happe last
            info!("Starting {}...", "happe".green());
            daemon::start_daemon("happe").await
                .context("Failed to start happe daemon")?;
            
            info!("{}", "All daemons started successfully".green().bold());
        }
        Commands::Stop => {
            println!("{}", "Stopping all daemons in reverse order...".bold());
            
            // Stop happe first
            info!("Stopping {}...", "happe".green());
            daemon::stop_daemon("happe").await
                .context("Failed to stop happe daemon")?;
            
            // Wait a moment for happe to terminate
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            
            // Stop ida next
            info!("Stopping {}...", "ida".green());
            daemon::stop_daemon("ida").await
                .context("Failed to stop ida daemon")?;
            
            // Wait a moment for ida to terminate
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            
            // Stop mcp-hostd last
            info!("Stopping {}...", "mcp-hostd".green());
            daemon::stop_daemon("mcp-hostd").await
                .context("Failed to stop mcp-hostd daemon")?;
            
            info!("{}", "All daemons stopped successfully".green().bold());
        }
    }
    
    Ok(())
} 