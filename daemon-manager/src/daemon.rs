use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use colored::Colorize;
use dirs::home_dir;
use std::fs;
use which::which;
use std::env;
use toml;

// Enum for representing daemon status
#[derive(Debug, Clone, PartialEq)]
pub enum DaemonStatus {
    Running,
    Stopped,
    NotInstalled,
    Unknown,
}

impl std::fmt::Display for DaemonStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DaemonStatus::Running => write!(f, "{}", "Running".green()),
            DaemonStatus::Stopped => write!(f, "{}", "Stopped".yellow()),
            DaemonStatus::NotInstalled => write!(f, "{}", "Not Installed".red()),
            DaemonStatus::Unknown => write!(f, "{}", "Unknown".red()),
        }
    }
}

// List of supported daemons
const SUPPORTED_DAEMONS: &[&str] = &["happe", "ida", "mcp-hostd"];

// Helper function to get the systemd service name for a daemon
fn get_service_name(daemon_name: &str) -> String {
    match daemon_name {
        "happe" => "gemini-happe".to_string(),
        "ida" => "gemini-ida".to_string(),
        "mcp-hostd" => "gemini-mcp-hostd".to_string(),
        _ => format!("gemini-{}", daemon_name),
    }
}

// Helper function to find the workspace root containing Cargo.toml
fn get_workspace_root() -> Result<PathBuf> {
    // First try the standard approach - find Cargo.toml in current or parent directories
    let current_dir = env::current_dir()
        .context("Failed to get current directory")?;
    
    if let Some(path) = current_dir
        .ancestors()
        .find(|p| p.join("Cargo.toml").exists()) {
        return Ok(path.to_path_buf());
    }
    
    // If that fails, check if we're installed and use a sensible default
    // First check if there's a config directory we can use as reference
    let config_dir = dirs::config_dir()
        .ok_or_else(|| anyhow!("Could not determine config directory"))?
        .join("gemini-suite");
    
    if config_dir.exists() {
        tracing::debug!("Using config directory as reference: {}", config_dir.display());
        return Ok(config_dir);
    }
    
    // Last resort, use the home directory
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow!("Could not determine home directory"))?;
    
    tracing::debug!("Using home directory as fallback workspace root: {}", home.display());
    Ok(home)
}

// Helper function to get the binary path for a daemon
fn get_binary_path(daemon_name: &str) -> Result<PathBuf> {
    let binary_name = match daemon_name {
        "happe" => "happe-daemon",  // Use full binary names for better matching
        "ida" => "ida-daemon",
        "mcp-hostd" => "mcp-hostd",
        _ => return Err(anyhow!("Unknown daemon: {}", daemon_name)),
    };
    
    // Try to find the binary in PATH first (most reliable when installed)
    if let Ok(path) = which(binary_name) {
        tracing::debug!("Found {} in PATH: {}", binary_name, path.display());
        return Ok(path);
    }
    
    // Also try with standard daemon names
    if daemon_name != binary_name {
        if let Ok(path) = which(daemon_name) {
            tracing::debug!("Found {} in PATH: {}", daemon_name, path.display());
            return Ok(path);
        }
    }
    
    // Check in cargo target directory relative to workspace root
    let workspace_root = get_workspace_root()?;
        
    let debug_path = workspace_root.join("target/debug").join(binary_name);
    if debug_path.exists() {
        return Ok(debug_path);
    }
    
    let release_path = workspace_root.join("target/release").join(binary_name);
    if release_path.exists() {
        return Ok(release_path);
    }
    
    // Check standard locations
    let local_bin = PathBuf::from("/usr/local/bin").join(binary_name);
    if local_bin.exists() {
        return Ok(local_bin);
    }
    
    let usr_bin = PathBuf::from("/usr/bin").join(binary_name);
    if usr_bin.exists() {
        return Ok(usr_bin);
    }
    
    // Check in ~/.local/bin
    if let Some(home) = dirs::home_dir() {
        let local_bin = home.join(".local/bin").join(binary_name);
        if local_bin.exists() {
            return Ok(local_bin);
        }
    }
    
    Err(anyhow!("Could not find binary for daemon: {}", daemon_name))
}

// Helper function to validate daemon name
fn validate_daemon_name(name: &str) -> Result<()> {
    if !SUPPORTED_DAEMONS.contains(&name) {
        return Err(anyhow!(
            "Unsupported daemon: {}. Supported daemons are: {}",
            name,
            SUPPORTED_DAEMONS.join(", ")
        ));
    }
    Ok(())
}

// Start a daemon
pub async fn start_daemon(name: &str) -> Result<()> {
    validate_daemon_name(name)?;
    
    let service_name = get_service_name(name);
    
    // Check if systemd service is installed
    if is_systemd_service_installed(&service_name)? {
        // Start using systemd
        tracing::debug!("Starting daemon {} via systemd", name);
        let status = Command::new("systemctl")
            .args(["--user", "start", &service_name])
            .status()
            .context("Failed to execute systemctl command")?;
            
        if !status.success() {
            return Err(anyhow!("Failed to start daemon {} via systemd", name));
        }
    } else {
        // Start manually
        tracing::debug!("Starting daemon {} manually", name);
        let binary_path = get_binary_path(name)?;
        
        // Use config directory as working directory
        let config_dir = dirs::config_dir()
            .ok_or_else(|| anyhow!("Could not determine config directory"))?
            .join("gemini-suite");
        
        // Ensure config directory exists
        if !config_dir.exists() {
            fs::create_dir_all(&config_dir)
                .context("Failed to create config directory")?;
            tracing::info!("Created configuration directory: {}", config_dir.display());
        }
        
        // Determine the command-line arguments based on the daemon
        let args: Vec<String> = match name {
            "happe" => vec![],
            "ida" => vec![],
            "mcp-hostd" => vec![],
            _ => vec![],
        };
        
        // Set environment variables
        let mut cmd = Command::new(binary_path);
        cmd.args(&args)
           .current_dir(&config_dir)
           .env("GEMINI_CONFIG_DIR", &config_dir)
           .stdout(Stdio::null())
           .stderr(Stdio::null());
        
        // Start the binary and detach
        let child = cmd.spawn()
            .with_context(|| format!("Failed to start daemon {}", name))?;
            
        tracing::debug!("Started {} with PID {}", name, child.id());
    }
    
    Ok(())
}

// Stop a daemon
pub async fn stop_daemon(name: &str) -> Result<()> {
    validate_daemon_name(name)?;
    
    let service_name = get_service_name(name);
    
    // Check if systemd service is installed
    if is_systemd_service_installed(&service_name)? {
        // Stop using systemd
        tracing::debug!("Stopping daemon {} via systemd", name);
        let status = Command::new("systemctl")
            .args(["--user", "stop", &service_name])
            .status()
            .context("Failed to execute systemctl command")?;
            
        if !status.success() {
            return Err(anyhow!("Failed to stop daemon {} via systemd", name));
        }
    } else {
        // Stop manually using killall
        tracing::debug!("Stopping daemon {} manually", name);
        let binary_name = match name {
            "happe" => "happe-daemon",
            "ida" => "ida-daemon",
            "mcp-hostd" => "mcp-hostd",
            _ => return Err(anyhow!("Unknown daemon: {}", name)),
        };
        
        let status = Command::new("killall")
            .arg(binary_name)
            .status()
            .context("Failed to execute killall command")?;
            
        if !status.success() {
            tracing::warn!("Failed to stop daemon {} using killall - it may not be running", name);
        }
    }
    
    Ok(())
}

// Restart a daemon
pub async fn restart_daemon(name: &str) -> Result<()> {
    validate_daemon_name(name)?;
    
    let service_name = get_service_name(name);
    
    // Check if systemd service is installed
    if is_systemd_service_installed(&service_name)? {
        // Restart using systemd
        tracing::debug!("Restarting daemon {} via systemd", name);
        let status = Command::new("systemctl")
            .args(["--user", "restart", &service_name])
            .status()
            .context("Failed to execute systemctl command")?;
            
        if !status.success() {
            return Err(anyhow!("Failed to restart daemon {} via systemd", name));
        }
    } else {
        // Manual stop and start
        stop_daemon(name).await?;
        start_daemon(name).await?;
    }
    
    Ok(())
}

// Check status of a daemon
pub async fn check_daemon_status(name: &str) -> Result<DaemonStatus> {
    validate_daemon_name(name)?;
    
    let service_name = get_service_name(name);
    
    // Check if systemd service is installed
    if is_systemd_service_installed(&service_name)? {
        // Check status using systemd
        tracing::debug!("Checking daemon {} status via systemd", name);
        let output = Command::new("systemctl")
            .args(["--user", "is-active", &service_name])
            .output()
            .context("Failed to execute systemctl command")?;
            
        let status_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        
        match status_str.as_str() {
            "active" => Ok(DaemonStatus::Running),
            "inactive" => Ok(DaemonStatus::Stopped),
            _ => Ok(DaemonStatus::Unknown),
        }
    } else {
        // Check if binary exists
        if get_binary_path(name).is_err() {
            return Ok(DaemonStatus::NotInstalled);
        }
        
        // Check manually using pgrep
        let binary_name = match name {
            "happe" => "happe-daemon",
            "ida" => "ida-daemon",
            "mcp-hostd" => "mcp-hostd",
            _ => return Err(anyhow!("Unknown daemon: {}", name)),
        };
        
        let status = Command::new("pgrep")
            .arg(binary_name)
            .status()
            .context("Failed to execute pgrep command")?;
            
        if status.success() {
            Ok(DaemonStatus::Running)
        } else {
            Ok(DaemonStatus::Stopped)
        }
    }
}

// List all daemons and their status
pub async fn list_daemons() -> Result<HashMap<String, DaemonStatus>> {
    let mut statuses = HashMap::new();
    
    for daemon in SUPPORTED_DAEMONS {
        let status = check_daemon_status(daemon).await?;
        statuses.insert(daemon.to_string(), status);
    }
    
    Ok(statuses)
}

// Helper function to check if a systemd service is installed
fn is_systemd_service_installed(service_name: &str) -> Result<bool> {
    let output = Command::new("systemctl")
        .args(["--user", "list-unit-files", "--type=service", service_name])
        .output()
        .context("Failed to execute systemctl command")?;
        
    let output_str = String::from_utf8_lossy(&output.stdout);
    
    // Check if service is listed
    Ok(output_str.contains(service_name))
}

// Install a daemon as a systemd service
pub async fn install_daemon(name: &str) -> Result<()> {
    validate_daemon_name(name)?;
    
    // Get binary path
    let binary_path = get_binary_path(name)?;
    let service_name = get_service_name(name);
    
    // Use config directory as working directory instead of workspace root
    // This is a more reliable location that will exist in both dev and install environments
    let config_dir = dirs::config_dir()
        .ok_or_else(|| anyhow!("Could not determine config directory"))?
        .join("gemini-suite");
    
    // Ensure config directory exists
    fs::create_dir_all(&config_dir)
        .context("Failed to create config directory")?;
    
    // Create systemd user service directory if it doesn't exist
    let systemd_dir = home_dir()
        .ok_or_else(|| anyhow!("Could not determine home directory"))?
        .join(".config/systemd/user");
    
    fs::create_dir_all(&systemd_dir)
        .context("Failed to create systemd user directory")?;
    
    // Load API key from the unified config if it exists
    let api_key_args = if name == "happe" {
        // Try to load the API key from unified config
        let config_path = config_dir.join("config.toml");
            
        if config_path.exists() {
            match fs::read_to_string(&config_path) {
                Ok(content) => {
                    match toml::from_str::<toml::Value>(&content) {
                        Ok(config) => {
                            // Extract API key from [gemini] section if it exists
                            let api_key = config.get("gemini")
                                .and_then(|g| g.get("api_key"))
                                .and_then(|k| k.as_str())
                                .unwrap_or("");
                                
                            if !api_key.is_empty() {
                                format!(" -k {}", api_key)
                            } else {
                                // No API key found, warn the user
                                tracing::warn!("No API key found in config.toml for {}", name);
                                String::new()
                            }
                        },
                        Err(e) => {
                            tracing::warn!("Failed to parse config.toml: {}", e);
                            String::new()
                        }
                    }
                },
                Err(e) => {
                    tracing::warn!("Failed to read config.toml: {}", e);
                    String::new()
                }
            }
        } else {
            tracing::warn!("No config.toml found at {}", config_path.display());
            String::new()
        }
    } else {
        // Not the happe daemon, no need for API key args
        String::new()
    };
    
    // Set environment variable for config directory
    let env_vars = format!("Environment=GEMINI_CONFIG_DIR={}", config_dir.display());
    
    // Create systemd service file
    let service_content = match name {
        "mcp-hostd" => {
            format!(
                r#"[Unit]
Description=Gemini Suite {} Daemon
After=network.target

[Service]
ExecStart={}{}
Restart=on-failure
RestartSec=5
WorkingDirectory={}
{}

[Install]
WantedBy=default.target
"#,
                name,
                binary_path.display(),
                api_key_args,
                config_dir.display(),
                env_vars
            )
        },
        "ida" => {
            format!(
                r#"[Unit]
Description=Gemini Suite {} Daemon
After=network.target
After=gemini-mcp-hostd.service
Requires=gemini-mcp-hostd.service

[Service]
ExecStart={}{}
Restart=on-failure
RestartSec=5
WorkingDirectory={}
{}

[Install]
WantedBy=default.target
"#,
                name,
                binary_path.display(),
                api_key_args,
                config_dir.display(),
                env_vars
            )
        },
        "happe" => {
            format!(
                r#"[Unit]
Description=Gemini Suite {} Daemon
After=network.target
After=gemini-mcp-hostd.service gemini-ida.service
Requires=gemini-mcp-hostd.service gemini-ida.service

[Service]
ExecStart={}{}
Restart=on-failure
RestartSec=5
WorkingDirectory={}
{}

[Install]
WantedBy=default.target
"#,
                name,
                binary_path.display(),
                api_key_args,
                config_dir.display(),
                env_vars
            )
        },
        _ => {
            format!(
                r#"[Unit]
Description=Gemini Suite {} Daemon
After=network.target

[Service]
ExecStart={}{}
Restart=on-failure
RestartSec=5
WorkingDirectory={}
{}

[Install]
WantedBy=default.target
"#,
                name,
                binary_path.display(),
                api_key_args,
                config_dir.display(),
                env_vars
            )
        }
    };
    
    let service_file = systemd_dir.join(format!("{}.service", service_name));
    fs::write(&service_file, service_content)
        .with_context(|| format!("Failed to write service file to {}", service_file.display()))?;
    
    // Reload systemd user daemon
    let status = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status()
        .context("Failed to reload systemd user daemon")?;
        
    if !status.success() {
        return Err(anyhow!("Failed to reload systemd user daemon"));
    }
    
    // Enable the service
    let status = Command::new("systemctl")
        .args(["--user", "enable", &service_name])
        .status()
        .context("Failed to enable systemd service")?;
        
    if !status.success() {
        return Err(anyhow!("Failed to enable systemd service"));
    }
    
    tracing::info!("Installed {} as a systemd user service", name);
    
    Ok(())
}

// Uninstall a daemon from systemd
pub async fn uninstall_daemon(name: &str) -> Result<()> {
    validate_daemon_name(name)?;
    
    let service_name = get_service_name(name);
    
    // Check if service is installed
    if !is_systemd_service_installed(&service_name)? {
        return Err(anyhow!("Daemon {} is not installed as a systemd service", name));
    }
    
    // Stop the service if it's running
    let status = check_daemon_status(name).await?;
    if status == DaemonStatus::Running {
        stop_daemon(name).await?;
    }
    
    // Disable the service
    let status = Command::new("systemctl")
        .args(["--user", "disable", &service_name])
        .status()
        .context("Failed to disable systemd service")?;
        
    if !status.success() {
        return Err(anyhow!("Failed to disable systemd service"));
    }
    
    // Remove the service file
    let service_file = home_dir()
        .ok_or_else(|| anyhow!("Could not determine home directory"))?
        .join(format!(".config/systemd/user/{}.service", service_name));
        
    if service_file.exists() {
        fs::remove_file(&service_file)
            .with_context(|| format!("Failed to remove service file {}", service_file.display()))?;
    }
    
    // Reload systemd user daemon
    let status = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status()
        .context("Failed to reload systemd user daemon")?;
        
    if !status.success() {
        return Err(anyhow!("Failed to reload systemd user daemon"));
    }
    
    tracing::info!("Uninstalled {} systemd user service", name);
    
    Ok(())
} 