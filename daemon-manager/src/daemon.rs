use anyhow::{anyhow, Context, Result};
use colored::Colorize;
use dirs::home_dir;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use which::which;

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
    let current_dir = env::current_dir().context("Failed to get current directory")?;

    if let Some(path) = current_dir
        .ancestors()
        .find(|p| p.join("Cargo.toml").exists())
    {
        return Ok(path.to_path_buf());
    }

    // If that fails, check if we're installed and use a sensible default
    // First check if there's a config directory we can use as reference
    let config_dir = dirs::config_dir()
        .ok_or_else(|| anyhow!("Could not determine config directory"))?
        .join("gemini-suite");

    if config_dir.exists() {
        tracing::debug!(
            "Using config directory as reference: {}",
            config_dir.display()
        );
        return Ok(config_dir);
    }

    // Last resort, use the home directory
    let home = dirs::home_dir().ok_or_else(|| anyhow!("Could not determine home directory"))?;

    tracing::debug!(
        "Using home directory as fallback workspace root: {}",
        home.display()
    );
    Ok(home)
}

// Helper function to get the binary path for a daemon
fn get_binary_path(daemon_name: &str) -> Result<PathBuf> {
    let binary_name = match daemon_name {
        "happe" => "happe-daemon", // Use full binary names for better matching
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
        tracing::debug!(
            "Found {} in debug path: {}",
            binary_name,
            debug_path.display()
        );
        return Ok(debug_path);
    }

    let release_path = workspace_root.join("target/release").join(binary_name);
    if release_path.exists() {
        tracing::debug!(
            "Found {} in release path: {}",
            binary_name,
            release_path.display()
        );
        return Ok(release_path);
    }

    // Check standard locations
    let local_bin = PathBuf::from("/usr/local/bin").join(binary_name);
    if local_bin.exists() {
        tracing::debug!(
            "Found {} in /usr/local/bin: {}",
            binary_name,
            local_bin.display()
        );
        return Ok(local_bin);
    }

    let usr_bin = PathBuf::from("/usr/bin").join(binary_name);
    if usr_bin.exists() {
        tracing::debug!("Found {} in /usr/bin: {}", binary_name, usr_bin.display());
        return Ok(usr_bin);
    }

    // Check in ~/.local/bin
    if let Some(home) = dirs::home_dir() {
        let local_bin = home.join(".local/bin").join(binary_name);
        if local_bin.exists() {
            tracing::debug!(
                "Found {} in ~/.local/bin: {}",
                binary_name,
                local_bin.display()
            );
            return Ok(local_bin);
        }
    }

    // Log all the paths we checked to help debugging
    tracing::error!("Could not find binary for daemon: {}. Tried:", daemon_name);
    tracing::error!("  - which {}", binary_name);
    tracing::error!("  - which {}", daemon_name);
    tracing::error!(
        "  - {}/target/debug/{}",
        workspace_root.display(),
        binary_name
    );
    tracing::error!(
        "  - {}/target/release/{}",
        workspace_root.display(),
        binary_name
    );
    tracing::error!("  - /usr/local/bin/{}", binary_name);
    tracing::error!("  - /usr/bin/{}", binary_name);
    if let Some(home) = dirs::home_dir() {
        tracing::error!("  - {}/.local/bin/{}", home.display(), binary_name);
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

// Helper function to get the runtime directory
fn get_runtime_dir() -> Result<PathBuf> {
    // First try XDG_RUNTIME_DIR
    let runtime_dir = dirs::runtime_dir()
        .or_else(|| {
            // Fallback to $HOME/.local/share if no runtime directory
            dirs::data_local_dir()
        })
        .ok_or_else(|| anyhow!("Could not determine runtime or local data directory"))?;

    let base_dir = runtime_dir.join("gemini-suite");

    // Ensure the directory exists
    if !base_dir.exists() {
        fs::create_dir_all(&base_dir).with_context(|| {
            format!("Failed to create runtime directory: {}", base_dir.display())
        })?;
    }

    Ok(base_dir)
}

// Helper function to check if a process exists
fn process_exists(pid: i32) -> bool {
    // Use kill with signal 0 to check process existence without actually killing it
    let status = Command::new("kill").args(["-0", &pid.to_string()]).status();

    match status {
        Ok(exit_status) => exit_status.success(),
        Err(_) => false, // Error running kill command, assume process doesn't exist
    }
}

// Helper function to kill a process
fn kill_process(pid: i32) -> Result<()> {
    let status = Command::new("kill")
        .arg(pid.to_string())
        .status()
        .with_context(|| format!("Failed to execute kill command for PID {}", pid))?;

    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("Kill command failed for PID {}", pid))
    }
}

// Helper function to get the binary name for a daemon
fn get_daemon_binary_name(name: &str) -> Result<&str> {
    match name {
        "happe" => Ok("happe-daemon"),
        "ida" => Ok("ida-daemon"),
        "mcp-hostd" => Ok("mcp-hostd"),
        _ => Err(anyhow!("Unknown daemon name: {}", name)),
    }
}

// Helper function to find PID using pgrep
fn find_pid_by_name(binary_name: &str) -> Result<Option<i32>> {
    let output = Command::new("pgrep")
        .arg("-f") // Match against full argument list
        .arg(binary_name)
        .output()
        .context("Failed to execute pgrep command")?;

    if output.status.success() {
        let pid_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        // pgrep might return multiple PIDs, take the first one
        if let Some(first_pid_str) = pid_str.lines().next() {
            let pid = first_pid_str.parse::<i32>().with_context(|| {
                format!("Failed to parse PID '{}' from pgrep output", first_pid_str)
            })?;
            Ok(Some(pid))
        } else {
            Ok(None) // pgrep succeeded but output was empty
        }
    } else {
        Ok(None) // pgrep failed (process not found)
    }
}

// Start a daemon
pub async fn start_daemon(name: &str) -> Result<()> {
    validate_daemon_name(name)?;

    let service_name = get_service_name(name);

    // Check status first to avoid starting if already running
    let current_status = check_daemon_status(name).await?;
    if current_status == DaemonStatus::Running {
        tracing::info!("Daemon {} is already running.", name);
        return Ok(());
    }

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
        tracing::info!("Started daemon {} via systemd.", name);
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
            fs::create_dir_all(&config_dir).context("Failed to create config directory")?;
            tracing::info!("Created configuration directory: {}", config_dir.display());
        }

        // Get runtime directory for PID file
        let runtime_dir = get_runtime_dir()?;
        let pid_file_path = runtime_dir.join(format!("{}.pid", name));

        // Create logs directory if it doesn't exist
        let logs_dir = runtime_dir.join("gemini-suite-logs");
        if !logs_dir.exists() {
            fs::create_dir_all(&logs_dir).context("Failed to create logs directory")?;
            tracing::info!("Created logs directory: {}", logs_dir.display());
        }

        // Create log file path
        let log_file = logs_dir.join(format!("{}.log", name));
        tracing::info!("Daemon logs will be written to: {}", log_file.display());

        // Determine the command-line arguments based on the daemon
        let args: Vec<String> = match name {
            "happe" => vec![],
            "ida" => vec![],
            "mcp-hostd" => vec![],
            _ => vec![],
        };

        // Set environment variable for unified config path
        let config_file_path = config_dir.join("config.toml");

        let mut cmd = Command::new(binary_path);
        cmd.args(&args)
            .current_dir(&config_dir)
            .env("GEMINI_SUITE_CONFIG_PATH", &config_file_path) // Use the correct env var
            .env("RUST_LOG", "debug") // Set RUST_LOG=debug for verbose logging
            .stdout(std::fs::File::create(&log_file).context("Failed to create log file")?)
            .stderr(std::fs::File::create(&log_file).context("Failed to create log file")?);

        // Start the binary and detach
        let child = cmd
            .spawn()
            .with_context(|| format!("Failed to start daemon {}", name))?;

        let pid = child.id() as i32;
        tracing::debug!("Started {} manually with PID {}", name, pid);

        // Write PID to file
        fs::write(&pid_file_path, pid.to_string())
            .with_context(|| format!("Failed to write PID file: {}", pid_file_path.display()))?;
        tracing::debug!("Wrote PID {} to file {}", pid, pid_file_path.display());
        tracing::info!("Started daemon {} manually.", name);
    }

    Ok(())
}

// Stop a daemon
pub async fn stop_daemon(name: &str) -> Result<()> {
    validate_daemon_name(name)?;

    let service_name = get_service_name(name);
    let binary_name = get_daemon_binary_name(name)?;

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
        // Stop manually: Try PID file first, then pgrep
        tracing::debug!("Stopping daemon {} manually", name);
        let runtime_dir = get_runtime_dir()?;
        let pid_file = runtime_dir.join(format!("{}.pid", name)); // Use simple name for pid file

        let mut pid_to_kill: Option<i32> = None;

        // 1. Try reading PID from file
        if pid_file.exists() {
            match fs::read_to_string(&pid_file) {
                Ok(pid_str) => match pid_str.trim().parse::<i32>() {
                    Ok(pid) => {
                        if process_exists(pid) {
                            tracing::debug!("Found running process PID {} from file {}", pid, pid_file.display());
                            pid_to_kill = Some(pid);
                        } else {
                            tracing::warn!("Stale PID file found: {}. Process {} not running. Removing file.", pid_file.display(), pid);
                            let _ = fs::remove_file(&pid_file); // Ignore error on removal
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to parse PID from file {}: {}. Ignoring file.",
                            pid_file.display(),
                            e
                        );
                    }
                },
                Err(e) => {
                    tracing::warn!(
                        "Failed to read PID file {}: {}. Ignoring file.",
                        pid_file.display(),
                        e
                    );
                }
            }
        }

        // 2. If PID not found via file, try pgrep
        if pid_to_kill.is_none() {
            tracing::debug!(
                "PID file for {} not found or invalid, attempting pgrep with binary name: {}",
                name,
                binary_name
            );
            match find_pid_by_name(binary_name)? {
                Some(pid) => {
                    tracing::info!("Found running process PID {} using pgrep for {}", pid, binary_name);
                    pid_to_kill = Some(pid);
                }
                None => {
                    tracing::info!(
                        "Daemon {} process not found using pgrep either. Assuming stopped.",
                        name
                    );
                    return Ok(()); // Daemon not running
                }
            }
        }

        // 3. Attempt to stop the process using the found PID
        if let Some(pid) = pid_to_kill {
            tracing::debug!("Attempting to stop process with PID: {}", pid);
            // Send SIGTERM first
            match kill_process(pid) {
                Ok(_) => {
                    tracing::debug!("SIGTERM sent successfully to process {}", pid);
                    // Optional: Wait briefly and check if process stopped, then send SIGKILL if needed
                    // For simplicity now, we assume SIGTERM works.

                    // Clean up PID file if it existed and we stopped the process from it
                    if pid_file.exists() {
                        if let Ok(pid_str) = fs::read_to_string(&pid_file) {
                            if pid_str.trim().parse::<i32>() == Ok(pid) {
                                if let Err(e) = fs::remove_file(&pid_file) {
                                    tracing::warn!("Failed to remove PID file {}: {}", pid_file.display(), e);
                                }
                            }
                        }
                    }

                    // Show message about viewing logs
                    let logs_dir = runtime_dir.join("gemini-suite-logs");
                    let log_file = logs_dir.join(format!("{}.log", name)); // Log file uses simple name
                    if log_file.exists() {
                        tracing::info!("Daemon logs available at: {}", log_file.display());
                        // tracing::info!("View them with: cat {}", log_file.display());
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to send SIGTERM to process {}: {}", pid, e);
                    return Err(anyhow!(
                        "Failed to stop daemon {} (PID: {}): {}",
                        name,
                        pid,
                        e
                    ));
                }
            }
        } else {
             // Should not happen if pgrep was attempted, but handle defensively
             tracing::warn!("Could not determine PID for daemon {}. Assuming stopped.", name);
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
    fs::create_dir_all(&config_dir).context("Failed to create config directory")?;

    // Create systemd user service directory if it doesn't exist
    let systemd_dir = home_dir()
        .ok_or_else(|| anyhow!("Could not determine home directory"))?
        .join(".config/systemd/user");

    fs::create_dir_all(&systemd_dir).context("Failed to create systemd user directory")?;

    // Load API key from the unified config if it exists (specifically for happe)
    let api_key_args = if name == "happe" {
        // Try to load the API key from unified config
        let config_path = config_dir.join("config.toml");

        if config_path.exists() {
            match fs::read_to_string(&config_path) {
                Ok(content) => {
                    match toml::from_str::<toml::Value>(&content) {
                        Ok(config) => {
                            // Extract API key from [gemini-api] section (the correct section)
                            let api_key = config
                                .get("gemini-api") // Use the correct table name
                                .and_then(|g| g.get("api_key"))
                                .and_then(|k| k.as_str())
                                .unwrap_or("");

                            if !api_key.is_empty() {
                                format!(" -k {}", api_key)
                            } else {
                                // No API key found, warn the user
                                tracing::warn!("No API key found in [gemini-api] section of config.toml for {}", name);
                                String::new()
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Failed to parse config.toml: {}", e);
                            String::new()
                        }
                    }
                }
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

    // Set environment variable for config path (should point to the file)
    let config_file_path = config_dir.join("config.toml");
    let env_vars = format!("Environment=GEMINI_SUITE_CONFIG_PATH={}", config_file_path.display());

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
        }
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
        }
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
        }
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
        return Err(anyhow!(
            "Daemon {} is not installed as a systemd service",
            name
        ));
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
