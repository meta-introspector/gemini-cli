use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};
use log::{debug, error, info, warn};
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;
use toml::Table;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Installation directory
    #[arg(short, long, default_value = "~/.local/bin")]
    install_dir: String,

    /// Configuration directory
    #[arg(short = 'c', long, default_value = "~/.config/gemini-suite")]
    config_dir: String,

    /// Verbose output
    #[arg(short, long, default_value_t = false)]
    verbose: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Install all components (CLI, Daemons, Manager)
    Install {
        /// Skip shell wrapper creation
        #[arg(long, default_value_t = false)]
        skip_shell_wrapper: bool,

        /// Skip installing the gemini-manager tool
        #[arg(long, default_value_t = false)]
        skip_manager: bool,
    },

    /// Uninstall all components
    Uninstall {
        /// Force removal of all components and configuration
        #[arg(short, long, default_value_t = false)]
        force: bool,

        /// Also uninstall the gemini-manager tool
        #[arg(long, default_value_t = false)]
        uninstall_manager: bool,
    },

    /// Update existing installation
    Update {
        /// Skip daemon updates
        #[arg(long, default_value_t = false)]
        skip_daemons: bool,

        /// Also update the gemini-manager tool
        #[arg(long, default_value_t = false)]
        update_manager: bool,
    },

    /// Install only the gemini-manager tool
    InstallManager,

    /// Uninstall only the gemini-manager tool
    UninstallManager,
}

/// Represents information about a daemon
#[derive(Debug, Clone)]
struct DaemonInfo {
    name: String,
    bin_name: String,
    description: String,
}

/// MCP Server configuration
#[derive(Debug, Serialize, Deserialize)]
struct McpServer {
    name: String,
    enabled: bool,
    transport: String,
    command: Vec<String>,
    args: Vec<String>,
    env: std::collections::HashMap<String, String>,
    auto_execute: Vec<String>,
}

impl Cli {
    fn get_install_dir(&self) -> Result<PathBuf> {
        let path_str = if self.install_dir.starts_with("~/") {
            match home::home_dir() {
                Some(path) => path.join(
                    self.install_dir
                        .strip_prefix("~/")
                        .ok_or_else(|| anyhow!("Invalid path prefix"))?,
                ),
                None => return Err(anyhow!("Could not resolve home directory")),
            }
        } else {
            PathBuf::from(&self.install_dir)
        };

        Ok(path_str)
    }

    fn get_config_dir(&self) -> Result<PathBuf> {
        let path_str = if self.config_dir.starts_with("~/") {
            match home::home_dir() {
                Some(path) => path.join(
                    self.config_dir
                        .strip_prefix("~/")
                        .ok_or_else(|| anyhow!("Invalid path prefix"))?,
                ),
                None => return Err(anyhow!("Could not resolve home directory")),
            }
        } else {
            PathBuf::from(&self.config_dir)
        };

        Ok(path_str)
    }
}

/// Helper function to determine the preferred runtime directory
fn get_runtime_dir() -> Result<PathBuf> {
    let base_dir = dirs::runtime_dir()
        .or_else(|| {
            // Fallback based on XDG Base Directory Specification if runtime_dir is None
            // $HOME/.local/share
            dirs::data_local_dir()
        })
        .ok_or_else(|| anyhow!("Could not determine runtime or local data directory"))?;
    let runtime_dir = base_dir.join("gemini-suite");
    fs::create_dir_all(&runtime_dir)?;
    Ok(runtime_dir)
}

/// Checks if a process with the given PID is running.
/// Sends signal 0, which checks for process existence without affecting it.
fn is_process_running(pid: i32) -> bool {
    match kill(Pid::from_raw(pid), None) {
        Ok(_) => true,                          // Signal 0 sent successfully, process exists
        Err(nix::errno::Errno::ESRCH) => false, // No such process
        Err(e) => {
            // Other errors might indicate permission issues, etc.
            // Log a warning but assume not running for safety.
            warn!(
                "Error checking status for PID {}: {}. Assuming not running.",
                pid, e
            );
            false
        }
    }
}

/// Attempts to stop a daemon process if it's running.
fn stop_daemon_if_running(daemon: &DaemonInfo, runtime_dir: &Path) -> Result<()> {
    info!("Checking status for daemon: {} ({})", daemon.bin_name, daemon.description);
    let pid_file_path = runtime_dir.join(format!("{}.pid", daemon.bin_name));

    if !pid_file_path.exists() {
        info!("  PID file not found, daemon likely not running.");
        return Ok(());
    }

    match fs::read_to_string(&pid_file_path) {
        Ok(pid_str) => {
            match pid_str.trim().parse::<i32>() {
                Ok(pid) => {
                    if is_process_running(pid) {
                        info!(
                            "  Daemon {} (PID {}) is running. Attempting graceful shutdown...",
                            daemon.bin_name, pid
                        );
                        // Try SIGTERM first
                        match kill(Pid::from_raw(pid), Some(Signal::SIGTERM)) {
                            Ok(_) => {
                                // Wait a bit for graceful shutdown
                                sleep(Duration::from_secs(2));
                                if is_process_running(pid) {
                                    warn!(
                                        "  Daemon {} (PID {}) did not stop gracefully. Sending SIGKILL...",
                                        daemon.bin_name,
                                        pid
                                    );
                                    // Force kill if still running
                                    if let Err(e) = kill(Pid::from_raw(pid), Some(Signal::SIGKILL))
                                    {
                                        error!("  Failed to send SIGKILL to PID {}: {}", pid, e);
                                        // Continue anyway, but log the error
                                    } else {
                                        info!(
                                            "  Daemon {} (PID {}) force-killed.",
                                            daemon.bin_name, pid
                                        );
                                    }
                                } else {
                                    info!(
                                        "  Daemon {} (PID {}) stopped gracefully.",
                                        daemon.bin_name, pid
                                    );
                                }
                            }
                            Err(nix::errno::Errno::ESRCH) => {
                                info!(
                                    "  Process {} (PID {}) already exited before SIGTERM.",
                                    daemon.bin_name, pid
                                );
                            }
                            Err(e) => {
                                error!("  Failed to send SIGTERM to PID {}: {}", pid, e);
                                // Attempt SIGKILL as fallback
                                warn!(
                                    "  Attempting SIGKILL for {} (PID {})...",
                                    daemon.bin_name, pid
                                );
                                if is_process_running(pid) {
                                    // Check again before SIGKILL
                                    if let Err(e_kill) =
                                        kill(Pid::from_raw(pid), Some(Signal::SIGKILL))
                                    {
                                        error!(
                                            "  Failed to send SIGKILL to PID {}: {}",
                                            pid, e_kill
                                        );
                                    } else {
                                        info!("  Daemon {} (PID {}) force-killed after SIGTERM error.", daemon.bin_name, pid);
                                    }
                                }
                            }
                        }
                    } else {
                        info!(
                            "  Stale PID file found for {} (PID {}). Process not running.",
                            daemon.bin_name, pid
                        );
                    }
                }
                Err(e) => {
                    warn!(
                        "  Failed to parse PID from {}: {}. Skipping stop attempt.",
                        pid_file_path.display(),
                        e
                    );
                }
            }
        }
        Err(e) => {
            warn!(
                "  Failed to read PID file {}: {}. Skipping stop attempt.",
                pid_file_path.display(),
                e
            );
        }
    }

    // Clean up the PID file regardless of whether the process was running
    info!("  Cleaning up PID file: {}", pid_file_path.display());
    if pid_file_path.exists() {
        if let Err(e) = fs::remove_file(&pid_file_path) {
            warn!(
                "  Failed to remove PID file {}: {}",
                pid_file_path.display(),
                e
            );
        } else {
            debug!("  Removed PID file successfully.");
        }
    }

    Ok(())
}

/// Prompts the user for input with a given message.
fn prompt_user_for_value(prompt_message: &str) -> Result<String> {
    let mut stdout = io::stdout();
    let stdin = io::stdin();
    print!("{} ", prompt_message);
    stdout.flush()?; // Ensure the prompt is displayed before reading

    let mut input = String::new();
    stdin.read_line(&mut input)?;
    Ok(input.trim().to_string())
}

/// Checks a TOML config file for a specific key (dot-separated path) and prompts if missing/empty.
fn check_and_prompt_for_config_value(
    config_path: &Path,
    key_path: &[&str],
    prompt_message: &str,
) -> Result<()> {
    if !config_path.exists() {
        warn!(
            "Config file {} not found for checking value. Skipping.",
            config_path.display()
        );
        return Ok(());
    }

    let config_content = fs::read_to_string(config_path)?;
    let mut config_value: toml::Value = config_content
        .parse()
        .map_err(|e| anyhow!("Failed to parse TOML from {}: {}", config_path.display(), e))?;

    // Function to recursively get a mutable reference to the value or check existence
    fn get_value_mut<'a>(
        current: &'a mut toml::Value,
        path: &[&str],
    ) -> Option<&'a mut toml::Value> {
        let mut node = current;
        for key in path {
            node = node.as_table_mut()?.get_mut(*key)?;
        }
        Some(node)
    }

    // Check if the key exists and its value is non-empty
    let needs_update = match get_value_mut(&mut config_value, key_path) {
        Some(value) => value.as_str().map_or(true, |s| s.trim().is_empty()),
        None => true, // Key path doesn't exist
    };

    if needs_update {
        info!(
            "Required configuration key '{}' is missing or empty in {}.",
            key_path.join("."),
            config_path.display()
        );
        let user_value = prompt_user_for_value(prompt_message)?;

        if user_value.is_empty() {
            return Err(anyhow!("Input for {} cannot be empty.", key_path.join(".")));
        }

        // Update the TOML value by navigating/creating tables
        let mut current_table = config_value
            .as_table_mut()
            .ok_or_else(|| anyhow!("TOML root is not a table in {}", config_path.display()))?;

        for (i, key) in key_path.iter().enumerate() {
            if i == key_path.len() - 1 {
                // Last key: insert the string value
                current_table.insert(key.to_string(), toml::Value::String(user_value.clone()));
            } else {
                // Intermediate key: ensure table exists and descend
                let entry = current_table
                    .entry(key.to_string())
                    .or_insert_with(|| toml::Value::Table(Table::new()));
                current_table = entry.as_table_mut().ok_or_else(|| {
                    anyhow!(
                        "Expected intermediate key '{}' to be a table in {}",
                        key,
                        config_path.display()
                    )
                })?;
            }
        }

        // Write the updated TOML back to the file
        // Use toml::to_string_pretty for better readability, though it might reorder things
        let updated_config_content = toml::to_string_pretty(&config_value)?;
        fs::write(config_path, updated_config_content)?;
        info!(
            "Updated {} in {}.",
            key_path.join("."),
            config_path.display()
        );
    }

    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Configure logging
    if cli.verbose {
        std::env::set_var("RUST_LOG", "debug");
    } else {
        std::env::set_var("RUST_LOG", "info");
    }
    env_logger::init();

    let install_dir = cli.get_install_dir()?;
    let config_dir = cli.get_config_dir()?;

    info!("Installation directory: {}", install_dir.display());
    info!("Configuration directory: {}", config_dir.display());

    // Create installation directory if it doesn't exist
    if !install_dir.exists() {
        fs::create_dir_all(&install_dir)?;
        info!("Created installation directory: {}", install_dir.display());
    }

    // Create configuration directory if it doesn't exist
    if !config_dir.exists() {
        fs::create_dir_all(&config_dir)?;
        info!("Created configuration directory: {}", config_dir.display());
    }

    // Define daemon information
    let daemons = vec![
        DaemonInfo {
            name: "MCP Host Daemon".to_string(),
            bin_name: "mcp-hostd".to_string(),
            description: "MCP Host Daemon for handling tool calls".to_string(),
        },
        DaemonInfo {
            name: "HAPPE Daemon".to_string(),
            bin_name: "happe-daemon".to_string(),
            description: "Host Application Environment Daemon".to_string(),
        },
        DaemonInfo {
            name: "IDA Daemon".to_string(),
            bin_name: "ida-daemon".to_string(),
            description: "Internal Dialogue App Daemon".to_string(),
        },
    ];

    // Define MCP servers
    let mcp_servers = vec!["filesystem-mcp", "command-mcp", "memory-store-mcp"];

    match &cli.command {
        Some(Commands::Install {
            skip_shell_wrapper,
            skip_manager,
        }) => {
            install_all(
                &install_dir,
                &config_dir,
                &daemons,
                &mcp_servers,
                !skip_shell_wrapper,
                !skip_manager,
            )?;
        }
        Some(Commands::Uninstall {
            force,
            uninstall_manager,
        }) => {
            uninstall_all(
                &install_dir,
                &config_dir,
                &daemons,
                &mcp_servers,
                *force,
                *uninstall_manager,
            )?;
        }
        Some(Commands::Update {
            skip_daemons,
            update_manager,
        }) => {
            update_all(
                &install_dir,
                &config_dir,
                &daemons,
                &mcp_servers,
                !skip_daemons,
                *update_manager,
            )?;
        }
        Some(Commands::InstallManager) => {
            install_manager(&install_dir)?;
        }
        Some(Commands::UninstallManager) => {
            uninstall_manager(&install_dir)?;
        }
        None => {
            // Default to install all including manager
            install_all(
                &install_dir,
                &config_dir,
                &daemons,
                &mcp_servers,
                true,
                true,
            )?;
        }
    }

    Ok(())
}

fn install_all(
    install_dir: &Path,
    config_dir: &Path,
    daemons: &[DaemonInfo],
    mcp_servers: &[&str],
    create_shell_wrappers: bool,
    install_manager_tool: bool,
) -> Result<()> {
    info!("Preparing for full installation...");
    let runtime_dir = get_runtime_dir()?;
    info!("Checking for and stopping running daemons...");
    for daemon in daemons {
        stop_daemon_if_running(daemon, &runtime_dir)?;
    }
    info!("Daemon check complete.");

    // Build all binaries
    build_binaries(true)?;

    // Install core binaries
    install_binary("gemini-cli", install_dir)?;
    for daemon in daemons {
        install_binary(&daemon.bin_name, install_dir)?;
    }
    for server in mcp_servers {
        install_binary(server, install_dir)?;
    }

    // Add this line to create symlinks for daemons
    create_daemon_symlinks(install_dir, daemons)?;

    // Install manager tool if requested
    if install_manager_tool {
        install_manager(install_dir)?;
    }

    // Create shell wrappers
    if create_shell_wrappers {
        create_cli_wrapper(install_dir, config_dir)?;
    }

    // Install unified configuration (will also create mcp_servers.json)
    install_unified_config(install_dir, config_dir, mcp_servers)?;

    // Check and prompt for essential config values
    info!("Checking configuration values...");
    check_and_prompt_for_config_value(
        &config_dir.join("config.toml"),
        &["gemini-api", "api_key"],
        "Enter your Gemini API Key:",
    )?;
    info!("Configuration checks complete.");

    info!("Installation complete!");
    info!("For CLI usage, reload your shell and type: gemini \"your prompt\"");
    if install_manager_tool {
        info!("Manage daemons and servers with: gemini-manager <command>");
    }

    Ok(())
}

fn build_binaries(build_manager: bool) -> Result<()> {
    info!("Building binaries (this may take a while)...");

    // List what we're going to build
    let mut binaries = vec![
        "gemini-cli",
        "mcp-hostd",
        "happe-daemon",
        "ida-daemon",
        "filesystem-mcp",
        "command-mcp",
        "memory-store-mcp",
    ];

    if build_manager {
        binaries.push("gemini-manager");
    }

    info!("Will build the following binaries: {:?}", binaries);

    // Build all binaries explicitly
    for binary in &binaries {
        info!("Building {}...", binary);

        let mut cmd = Command::new("cargo");
        cmd.arg("build")
           .arg("--release")
           .arg("--bin")
           .arg(binary);

        // Add required features for specific binaries
        if binary == &"ida-daemon" {
            cmd.arg("--features").arg("gemini-mcp");
        }

        let output = cmd.output()?;

        if !output.status.success() {
            let error_msg = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("Failed to build {}: {}", binary, error_msg));
        }
        debug!("{} built successfully", binary);
    }

    // Verify that all binaries were built
    for binary in &binaries {
        let binary_path = Path::new("target/release").join(binary);
        if !binary_path.exists() {
            return Err(anyhow!(
                "Binary not found after build: {}",
                binary_path.display()
            ));
        }
        info!("Verified binary exists: {}", binary_path.display());
    }

    info!("All requested binaries built successfully");
    Ok(())
}

fn install_binary(bin_name: &str, install_dir: &Path) -> Result<()> {
    info!("Installing {} binary...", bin_name);

    let src_path = Path::new("target/release").join(bin_name);
    let dst_path = install_dir.join(bin_name);

    if !src_path.exists() {
        // List files in target/release to help troubleshoot
        let dir_path = Path::new("target/release");
        if dir_path.exists() {
            info!("Listing contents of {}:", dir_path.display());
            let entries = match fs::read_dir(dir_path) {
                Ok(entries) => entries,
                Err(e) => {
                    return Err(anyhow!(
                        "Could not read directory {}: {}",
                        dir_path.display(),
                        e
                    ));
                }
            };

            for entry in entries {
                if let Ok(entry) = entry {
                    info!("  {}", entry.path().display());
                }
            }
        } else {
            info!("Directory {} does not exist", dir_path.display());
        }

        return Err(anyhow!("Binary not found: {}", src_path.display()));
    }

    info!(
        "Copying from {} to {}",
        src_path.display(),
        dst_path.display()
    );
    fs::copy(&src_path, &dst_path)?;

    // Make executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&dst_path)?.permissions();
        perms.set_mode(0o755); // rwxr-xr-x
        fs::set_permissions(&dst_path, perms)?;
    }

    info!("Installed {} to {}", bin_name, dst_path.display());
    Ok(())
}

fn create_cli_wrapper(install_dir: &Path, config_dir: &Path) -> Result<()> {
    info!("Creating CLI wrapper function...");

    let shell_config_path = get_shell_config_path()?;
    let cli_bin_path = install_dir.join("gemini-cli");

    // Check if wrapper already exists
    if shell_config_exists_with_pattern(&shell_config_path, "# Gemini CLI Wrapper Function Start")?
    {
        remove_shell_function(
            &shell_config_path,
            "# Gemini CLI Wrapper Function Start",
            "# Gemini CLI Wrapper Function End",
        )?;
    }

    let wrapper_content = format!(
        r#"
# Gemini CLI Wrapper Function Start
# This function wraps the gemini-cli
gemini() {{
    # The binary name is expanded by install script HERE
    local gemini_bin="{}"
    # The config directory is expanded by install script HERE
    local config_dir="{}"

    if [ ! -x "$gemini_bin" ]; then
        # The binary name is expanded by install script HERE
        echo "Error: gemini-cli not found or not executable at [$gemini_bin]" >&2
        return 1
    fi

    # Create or use an existing session ID for persistence
    # Only generate a new one if --new-session is specified
    if [[ "$*" == *--new-session* ]]; then
        # Generate a new session ID for --new-session
        unset GEMINI_SESSION_ID
    fi

    # If GEMINI_SESSION_ID is not set, generate one
    if [ -z "${{GEMINI_SESSION_ID}}" ]; then
        # Use timestamp + terminal PID as a simple session ID
        local timestamp=$(date +%s)
        local ppid=$(ps -o ppid= -p $$)
        export GEMINI_SESSION_ID="term_${{ppid}}_${{timestamp}}"
        if [[ "$*" != *--set-api-key* ]] && 
           [[ "$*" != *--set-system-prompt* ]] && 
           [[ "$*" != *--show-config* ]] && 
           [[ "$*" != *--select-session* ]]; then
            echo "Started new conversation (session: $GEMINI_SESSION_ID)"
        fi
    fi

    # Set the config directory environment variable
    export GEMINI_SUITE_CONFIG_PATH="$config_dir/config.toml"

    # Simply execute the binary with all arguments passed to the function
    # Pass the GEMINI_SESSION_ID environment variable implicitly
    "$gemini_bin" "$@"
    return $? # Return the exit code of the binary
}}
# Gemini CLI Wrapper Function End
"#,
        cli_bin_path.display(),
        config_dir.display()
    );

    // Append the wrapper function to the shell config file
    append_to_shell_config(&shell_config_path, &wrapper_content)?;

    println!(
        "Gemini CLI wrapper function added to shell config: {}",
        shell_config_path.display()
    );
    println!("You'll need to restart your shell or run 'source {}' to use it", shell_config_path.display());

    Ok(())
}

fn install_unified_config(
    install_dir: &Path,
    config_dir: &Path,
    mcp_servers: &[&str],
) -> Result<()> {
    info!("Installing unified configuration...");

    fs::create_dir_all(config_dir)?;

    let config_path = config_dir.join("config.toml");
    let mcp_config_path = config_dir.join("mcp_servers.json"); // Define path for MCP JSON

    // Only create the config if it doesn't exist
    if !config_path.exists() {
        info!("Unified config.toml not found. Creating default...");
        // Determine default socket paths based on runtime directory
        let runtime_dir = get_runtime_dir()?;
        let ida_socket_path = runtime_dir.join("ida-daemon.sock");
        let happe_socket_path = runtime_dir.join("happe-daemon.sock");
        let mcp_socket_path = runtime_dir.join("mcp-hostd.sock"); // For reference only

        // Create directories needed for the configuration (history, memory DB)
        let history_dir = config_dir.join("history");
        let memory_dir = config_dir.join("memory"); // Base directory for memory data

        fs::create_dir_all(&history_dir)?;
        fs::create_dir_all(&memory_dir)?;

        info!("Created history directory at {}", history_dir.display());
        info!("Created memory directory base at {}", memory_dir.display());

        // Create a proper default config.toml with all necessary sections
        let default_config = format!(
            r#"# Gemini Suite Configuration

[gemini-api]
# Set your Gemini API key
api_key = ""
# Model to use for interactions
model_name = "gemini-2.5-pro-preview-03-25"
# System prompt to use for interactions
system_prompt = "You are a helpful assistant. Answer the user's questions concisely and accurately."
# Whether to save history
save_history = true
# Whether to enable memory broker
enable_memory_broker = true
# Whether to enable automatic memory storage
enable_auto_memory = true
# Model for memory broker operations (typically smaller/faster than main model)
memory_broker_model = "gemini-2.0-flash"

[cli]
# Optional custom path to history file
# history_file_path = ""
# Default log level
log_level = "info"
# Path to HAPPE socket (detected automatically if empty)
# happe_ipc_path = "{}"

[happe]
# Path to IDA daemon socket (detected automatically if empty)
# ida_socket_path = "{}"
# Path to HAPPE daemon socket (detected automatically if empty)
# happe_socket_path = "{}"
# Whether HTTP server is enabled
http_enabled = false
# Bind address for HTTP server if enabled
http_bind_addr = "127.0.0.1:3000"
# System prompt for HAPPE interactions
# system_prompt = ""

[ida]
# Path to IDA daemon socket (detected automatically if empty)
# ida_socket_path = "{}"
# Path to memory database
memory_db_path = "{}/memory/db"
# Maximum number of memory results to return
max_memory_results = 5
# Semantic similarity threshold for memory retrieval
semantic_similarity_threshold = 0.7

[memory]
# Path to the LanceDB database
db_path = "{}/memory/db"
# Embedding model to use
embedding_model = "e5"
# Embedding model variant to use
embedding_model_variant = "e5-small-v2"
# Path to storage for embeddings/models
storage_path = "{}/memory/models"

[mcp]
# Path to MCP servers config file
# mcp_servers_file_path = "{}"
# Path to MCP host daemon socket
# mcp_host_socket_path = "{}"

[daemon-manager]
# Where to install daemon executables
# daemon_install_path = "{}"
"#,
            happe_socket_path.to_string_lossy(),
            ida_socket_path.to_string_lossy(),
            happe_socket_path.to_string_lossy(),
            ida_socket_path.to_string_lossy(),
            memory_dir.to_string_lossy(),
            memory_dir.to_string_lossy(),
            memory_dir.to_string_lossy(),
            mcp_config_path.to_string_lossy(),
            mcp_socket_path.to_string_lossy(),
            install_dir.to_string_lossy()
        );

        // Write the default config
        fs::write(&config_path, default_config)?;
        info!(
            "Created default unified config.toml at {}",
            config_path.display()
        );

        // Create the default mcp_servers.json file separately with the newer format
        // Use Claude-compatible format with mcpServers object
        let mut mcp_server_map = std::collections::HashMap::new();

        for &server_name in mcp_servers {
            let server_path = install_dir.join(server_name);
            let auto_execute = match server_name {
                "memory-store-mcp" => vec![
                    "store_memory".to_string(),
                    "list_all_memories".to_string(),
                    "retrieve_memory_by_key".to_string(),
                    "retrieve_memory_by_tag".to_string(),
                    "delete_memory_by_key".to_string(),
                ],
                "embedding-server-mcp" => vec!["get_embeddings".to_string()],
                _ => Vec::new(),
            };

            let mut env = std::collections::HashMap::new();
            env.insert("GEMINI_MCP_TIMEOUT".to_string(), "120".to_string());

            let server_config = serde_json::json!({
                "command": server_path.to_string_lossy().to_string(),
                "args": [],
                "env": env,
                "enabled": true,
                "auto_execute": auto_execute
            });

            let name = server_name.trim_end_matches("-mcp").to_string();
            mcp_server_map.insert(name, server_config);
        }

        let servers_container = serde_json::json!({
            "mcpServers": mcp_server_map
        });

        let mcp_servers_json_content = serde_json::to_string_pretty(&servers_container)
            .map_err(|e| anyhow!("Failed to serialize MCP server config to JSON: {}", e))?;
        fs::write(&mcp_config_path, mcp_servers_json_content)?;
        info!(
            "Created default MCP server configuration at {}",
            mcp_config_path.display()
        );
    } else {
        info!(
            "Unified configuration already exists at {}",
            config_path.display()
        );
        // Ensure mcp_servers.json also exists if config.toml does
        if !mcp_config_path.exists() {
            warn!("Unified config.toml exists, but mcp_servers.json is missing. Creating default.");

            // Use Claude-compatible format with mcpServers object
            let mut mcp_server_map = std::collections::HashMap::new();

            for &server_name in mcp_servers {
                let server_path = install_dir.join(server_name);
                let auto_execute = match server_name {
                    "memory-store-mcp" => vec![
                        "store_memory".to_string(),
                        "list_all_memories".to_string(),
                        "retrieve_memory_by_key".to_string(),
                        "retrieve_memory_by_tag".to_string(),
                        "delete_memory_by_key".to_string(),
                    ],
                    "embedding-server-mcp" => vec!["get_embeddings".to_string()],
                    _ => Vec::new(),
                };

                let mut env = std::collections::HashMap::new();
                env.insert("GEMINI_MCP_TIMEOUT".to_string(), "120".to_string());

                let server_config = serde_json::json!({
                    "command": server_path.to_string_lossy().to_string(),
                    "args": [],
                    "env": env,
                    "enabled": true,
                    "auto_execute": auto_execute
                });

                let name = server_name.trim_end_matches("-mcp").to_string();
                mcp_server_map.insert(name, server_config);
            }

            // Create the servers container object
            let servers_container = serde_json::json!({
                "mcpServers": mcp_server_map
            });

            let mcp_servers_json_content = serde_json::to_string_pretty(&servers_container)
                .map_err(|e| anyhow!("Failed to serialize MCP server config to JSON: {}", e))?;
            fs::write(&mcp_config_path, mcp_servers_json_content)?;
            info!(
                "Created default MCP server configuration at {}",
                mcp_config_path.display()
            );
        }
    }

    Ok(())
}

// Renamed struct to indicate it's only for JSON serialization
#[derive(Debug, Serialize, Deserialize, Clone)]
struct McpServerConfigForJson {
    name: String,
    enabled: bool,
    transport: String,
    command: Vec<String>,
    args: Vec<String>,
    env: std::collections::HashMap<String, String>,
    auto_execute: Vec<String>,
}

fn uninstall_all(
    install_dir: &Path,
    config_dir: &Path,
    daemons: &[DaemonInfo],
    mcp_servers: &[&str],
    force: bool,
    uninstall_manager_tool: bool,
) -> Result<()> {
    info!("Uninstalling core components...");
    // Remove core binaries
    uninstall_binary("gemini-cli", install_dir)?;
    for daemon in daemons {
        uninstall_binary(&daemon.bin_name, install_dir)?;
    }
    for server in mcp_servers {
        uninstall_binary(server, install_dir)?;
    }

    // Remove manager if requested
    if uninstall_manager_tool {
        uninstall_manager(install_dir)?;
    }

    // Remove shell wrapper functions
    let shell_config_path = get_shell_config_path()?;
    info!(
        "Removing shell wrapper functions from {}...",
        shell_config_path.display()
    );
    remove_shell_function(
        &shell_config_path,
        "# Gemini CLI Wrapper Function Start",
        "# Gemini CLI Wrapper Function End",
    )?;
    info!("Shell wrapper functions removed.");

    // Remove configuration if force flag is set
    if force {
        if config_dir.exists() {
            info!(
                "Removing unified configuration directory (--force specified): {}",
                config_dir.display()
            );
            fs::remove_dir_all(config_dir)?;
            info!("Removed configuration directory: {}", config_dir.display());
        } else {
            info!(
                "Configuration directory {} does not exist, skipping removal.",
                config_dir.display()
            );
        }

        // Check for runtime data and remove if force is set
        if let Ok(runtime_dir) = get_runtime_dir() {
            if runtime_dir.exists() {
                info!(
                    "Removing runtime data directory (--force specified): {}",
                    runtime_dir.display()
                );
                fs::remove_dir_all(&runtime_dir)?;
                info!("Removed runtime data directory: {}", runtime_dir.display());
            } else {
                info!(
                    "Runtime data directory {} does not exist, skipping removal.",
                    runtime_dir.display()
                );
            }
        } else {
            warn!("Could not determine runtime directory, skipping its removal.");
        }
    } else {
        info!(
            "Configuration directory {} and runtime data were preserved.",
            config_dir.display()
        );
        info!("Use the '--force' flag during uninstall to remove them.");
    }

    info!("Uninstallation complete!");

    Ok(())
}

fn update_all(
    install_dir: &Path,
    config_dir: &Path,
    daemons: &[DaemonInfo],
    mcp_servers: &[&str],
    update_daemons: bool,
    update_manager_tool: bool,
) -> Result<()> {
    info!("Preparing for update...");
    let runtime_dir = get_runtime_dir()?;
    info!("Checking for and stopping running daemons...");
    for daemon in daemons {
        if update_daemons || (update_manager_tool && daemon.bin_name == "mcp-hostd") {
            stop_daemon_if_running(daemon, &runtime_dir)?;
        } else {
            if daemon.bin_name == "mcp-hostd" {
                // Always stop mcpd if updating MCP servers
                info!("Stopping MCP Host Daemon to allow MCP server binary updates...");
                stop_daemon_if_running(daemon, &runtime_dir)?;
            }
        }
    }
    info!("Daemon check complete.");

    // Build binaries
    build_binaries(update_manager_tool)?;

    // Update binaries
    install_binary("gemini-cli", install_dir)?;
    if update_daemons {
        for daemon in daemons {
            install_binary(&daemon.bin_name, install_dir)?;
        }
    } else {
        // Even if not updating daemons, update MCP servers
        info!("Updating MCP server binaries...");
        for server in mcp_servers {
            install_binary(server, install_dir)?;
        }
    }

    // Update manager if requested
    if update_manager_tool {
        install_manager(install_dir)?;
    }

    // Update shell wrappers
    create_cli_wrapper(install_dir, config_dir)?;

    // Ensure unified config exists (will create if missing)
    install_unified_config(install_dir, config_dir, mcp_servers)?;

    // Check and prompt for essential config values
    info!("Checking configuration values...");
    check_and_prompt_for_config_value(
        &config_dir.join("config.toml"),
        &["gemini-api", "api_key"],
        "Enter your Gemini API Key:",
    )?;
    info!("Configuration checks complete.");

    info!("Update complete!");
    info!("Configuration location: {}", config_dir.display());

    Ok(())
}

fn uninstall_binary(bin_name: &str, install_dir: &Path) -> Result<()> {
    let bin_path = install_dir.join(bin_name);

    if bin_path.exists() {
        fs::remove_file(&bin_path)?;
        info!("Removed {}", bin_path.display());
    } else {
        debug!("Binary not found: {}", bin_path.display());
    }

    Ok(())
}

fn get_shell_config_path() -> Result<PathBuf> {
    let home_dir = home::home_dir().ok_or_else(|| anyhow!("Could not determine home directory"))?;

    // Determine shell type
    let shell = std::env::var("SHELL").unwrap_or_else(|_| String::from("/bin/bash"));

    let config_path = if shell.contains("zsh") {
        home_dir.join(".zshrc")
    } else if shell.contains("bash") {
        home_dir.join(".bashrc")
    } else {
        return Err(anyhow!("Unsupported shell: {}", shell));
    };

    Ok(config_path)
}

fn shell_config_exists_with_pattern(path: &Path, pattern: &str) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }

    let mut file = File::open(path)?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;

    Ok(content.contains(pattern))
}

fn remove_shell_function(path: &Path, start_marker: &str, end_marker: &str) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    let mut file = File::open(path)?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;

    if !content.contains(start_marker) {
        return Ok(());
    }

    info!("Removing existing shell function from {}", path.display());

    // Split the content by lines
    let lines: Vec<&str> = content.lines().collect();
    let mut result = Vec::new();

    let mut skip = false;
    for line in lines {
        if line.trim() == start_marker {
            skip = true;
            continue;
        }

        if line.trim() == end_marker {
            skip = false;
            continue;
        }

        if !skip {
            result.push(line);
        }
    }

    let new_content = result.join("\n");
    fs::write(path, new_content)?;

    Ok(())
}

fn append_to_shell_config(path: &Path, content: &str) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .append(true)
        .create(true)
        .open(path)?;

    file.write_all(content.as_bytes())?;

    Ok(())
}

// *** New functions for manager installation ***

fn install_manager(install_dir: &Path) -> Result<()> {
    info!("Installing Gemini Manager tool...");

    // 1. Build the manager binary (ensure build_binaries was called or call it specifically)
    // Assuming build_binaries(true) was called by the caller
    info!("Ensuring gemini-manager binary is built...");
    let manager_bin_path = Path::new("target/release").join("gemini-manager");
    if !manager_bin_path.exists() {
        warn!("gemini-manager binary not found, attempting to build it now...");
        let output = Command::new("cargo")
            .arg("build")
            .arg("--release")
            .arg("--bin")
            .arg("gemini-manager")
            .output()?;
        if !output.status.success() {
            return Err(anyhow!(
                "Failed to build gemini-manager: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }

    // 2. Install the manager binary
    install_binary("gemini-manager", install_dir)?;

    // 3. Ensure install directory is in PATH
    ensure_path_contains(install_dir)?;

    info!("Gemini Manager installed successfully.");
    info!("Run 'gemini-manager --help' to see available commands.");

    Ok(())
}

fn uninstall_manager(install_dir: &Path) -> Result<()> {
    info!("Uninstalling Gemini Manager tool...");
    uninstall_binary("gemini-manager", install_dir)?;
    info!("Gemini Manager uninstalled successfully.");
    Ok(())
}

/// Checks if the given directory is in the PATH and adds it if not.
fn ensure_path_contains(dir_to_add: &Path) -> Result<()> {
    let dir_str = dir_to_add
        .to_str()
        .ok_or_else(|| anyhow!("Install directory path is not valid UTF-8"))?;

    let current_path = env::var("PATH").unwrap_or_default();

    if current_path.split(':').any(|p| Path::new(p) == dir_to_add) {
        debug!("Install directory {} is already in PATH.", dir_str);
        return Ok(());
    }

    info!(
        "Install directory {} not found in PATH. Attempting to add it...",
        dir_str
    );

    let shell_config_path = get_shell_config_path()?;
    let shell = std::env::var("SHELL").unwrap_or_else(|_| String::from("/bin/bash"));

    let path_addition_line = if shell.contains("fish") {
        format!("fish_add_path {}", dir_str)
    } else {
        // Generic POSIX shell export - escape the $ for PATH
        format!("export PATH=\"{}:\\$PATH\"", dir_str)
    };

    let marker_start = "# Gemini Suite PATH Modification Start";
    let marker_end = "# Gemini Suite PATH Modification End";

    // Check if our specific modification already exists
    if shell_config_exists_with_pattern(&shell_config_path, &path_addition_line)? {
        info!(
            "PATH modification for {} already exists in {}.",
            dir_str,
            shell_config_path.display()
        );
        return Ok(());
    }

    // Remove any previous attempts (using markers)
    remove_shell_function(&shell_config_path, marker_start, marker_end)?;

    // Append the new modification with markers
    let content_to_append = format!(
        "\n{}\n{}\n{}\n",
        marker_start, path_addition_line, marker_end
    );
    append_to_shell_config(&shell_config_path, &content_to_append)?;

    info!(
        "Added {} to PATH in {}.",
        dir_str,
        shell_config_path.display()
    );
    warn!(
        "You may need to restart your shell or run 'source {}' for changes to take effect.",
        shell_config_path.display()
    );

    Ok(())
}

// New function to create symlinks for daemon binaries
fn create_daemon_symlinks(install_dir: &Path, daemons: &[DaemonInfo]) -> Result<()> {
    info!("Creating daemon symlinks...");
    for daemon in daemons {
        info!("Creating symlink for {} ({})", daemon.bin_name, daemon.description);
        // Only create symlinks if binary name doesn't match daemon name
        if daemon.bin_name != daemon.name {
            let binary_path = install_dir.join(&daemon.bin_name);
            let symlink_path = install_dir.join(&daemon.name);

            // Skip if symlink already exists and points to the correct target
            if symlink_path.exists() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::symlink;

                    // If it's already a symlink pointing to the correct target, skip
                    if let Ok(target) = fs::read_link(&symlink_path) {
                        if target == binary_path {
                            info!("Symlink for {} already exists and is correct", daemon.name);
                            continue;
                        }
                    }

                    // If it exists but is not a symlink or points to wrong target, remove it
                    if symlink_path.exists() {
                        info!(
                            "Removing existing file/symlink at {}",
                            symlink_path.display()
                        );
                        fs::remove_file(&symlink_path)?;
                    }

                    // Create the symlink
                    info!(
                        "Creating symlink: {} -> {}",
                        symlink_path.display(),
                        binary_path.display()
                    );
                    symlink(&binary_path, &symlink_path).map_err(|e| {
                        anyhow!("Failed to create symlink for {}: {}", daemon.name, e)
                    })?;
                }

                #[cfg(not(unix))]
                {
                    warn!(
                        "Symlink creation not supported on this platform for {}",
                        daemon.name
                    );
                    warn!("Please manually create an alias or copy the binary.");
                }
            }
        }
    }

    info!("Daemon symlinks created successfully");
    Ok(())
}
