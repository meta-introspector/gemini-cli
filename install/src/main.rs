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
        Some(value) => value.as_str().is_none_or(|s| s.trim().is_empty()),
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
    info!("Starting installation...");

    // 1. Stop running daemons (if any)
    let runtime_dir = get_runtime_dir()?;
    for daemon in daemons {
        stop_daemon_if_running(daemon, &runtime_dir)?;
    }

    // 2. Build necessary binaries
    build_binaries(install_manager_tool)?;

    // 3. Install CLI binary
    install_binary("gemini-cli", install_dir)?;

    // 4. Install Daemon binaries
    for daemon in daemons {
        install_binary(&daemon.bin_name, install_dir)?;
    }

    // 5. Install Manager Tool (if requested)
    if install_manager_tool {
        install_manager(install_dir)?;
    } else {
        info!("Skipping gemini-manager installation.");
    }

    // 6. Copy python_mcp directory
    let python_mcp_source_path = PathBuf::from("python_mcp");
    if python_mcp_source_path.is_dir() {
        // Install python scripts into a subdir, e.g., install_dir/python_mcp
        let python_mcp_dest_path = install_dir.join("python_mcp");
        info!(
            "Copying Python MCP server implementation to {}...",
            python_mcp_dest_path.display()
        );
        copy_dir_all(&python_mcp_source_path, &python_mcp_dest_path)?;
    } else {
        warn!(
            "Source directory 'python_mcp' not found, skipping Python MCP server installation."
        );
    }

    // 7. Create/Update Unified Configuration
    install_unified_config(install_dir, config_dir, mcp_servers)?;

    // 8. Ensure install_dir is in PATH
    ensure_path_contains(install_dir)?;

    // 9. Create shell wrappers (if requested)
    if create_shell_wrappers {
        create_cli_wrapper(install_dir, config_dir)?;
    } else {
        info!("Skipping shell wrapper creation.");
    }

    info!("Installation completed successfully!");
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

            // Use flatten to simplify iteration over Ok results
            for entry in entries.flatten() {
                info!("  {}", entry.path().display());
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

    // Create default unified config if it doesn't exist
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
    } else {
        info!(
            "Unified configuration already exists at {}",
            config_path.display()
        );
    }
    
    // Always generate and write/overwrite mcp_servers.json during install/update
    info!("Generating/Updating MCP server configuration at {}", mcp_config_path.display());
    let mut mcp_server_configs = Vec::new();
    for server_name in mcp_servers {
        // Construct the path to the python script
        let script_name = if *server_name == "memory-store-mcp" {
            "memstore.py".to_string()
        } else {
            format!("{}_mcp.py", server_name.replace("-mcp", ""))
        };
        let script_path = install_dir.join("python_mcp").join("servers").join(&script_name);
        
        // Command is the explicit python3 interpreter path
        let command = vec![
            "/usr/bin/python3".to_string()
        ];
        // Argument is the script path
        let args = vec![
            script_path.to_string_lossy().to_string()
        ];
        
        info!(
            "Generating config for Python MCP server: {} -> Command: {:?}, Args: {:?}",
            server_name,
            command,
            args
        );

        mcp_server_configs.push(McpServerConfigForJson {
            name: server_name.to_string(),
            enabled: true,
            transport: "stdio".to_string(),
            command,
            args,   
            env: std::collections::HashMap::new(),
            auto_execute: vec![],
        });
    }

    // Write the MCP server configuration JSON file directly as an array, overwriting if exists
    let mcp_servers_json_content = serde_json::to_string_pretty(&mcp_server_configs)
        .map_err(|e| anyhow!("Failed to serialize MCP server config to JSON: {}", e))?;
    fs::write(&mcp_config_path, mcp_servers_json_content)?;
    info!(
        "Wrote MCP server configuration to {}",
        mcp_config_path.display()
    );

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
    info!("Starting uninstallation...");

    // 1. Stop running daemons
    let runtime_dir = get_runtime_dir()?;
    for daemon in daemons {
        stop_daemon_if_running(daemon, &runtime_dir)?;
    }

    // 2. Remove CLI binary
    uninstall_binary("gemini-cli", install_dir)?;

    // 3. Remove Daemon binaries
    for daemon in daemons {
        uninstall_binary(&daemon.bin_name, install_dir)?;
    }

    // 4. Remove Manager Tool (if requested)
    if uninstall_manager_tool {
        uninstall_manager(install_dir)?;
    } else {
        info!("Skipping gemini-manager uninstallation.");
    }

    // 5. Remove python_mcp directory
    let python_mcp_dest_path = install_dir.join("python_mcp");
    if python_mcp_dest_path.is_dir() {
        info!("Removing Python MCP server directory: {}", python_mcp_dest_path.display());
        if let Err(e) = fs::remove_dir_all(&python_mcp_dest_path) {
            error!("Failed to remove {}: {}. Manual removal may be required.", python_mcp_dest_path.display(), e);
        } else {
            info!("Removed {}.", python_mcp_dest_path.display());
        }
    }

    // 6. Remove shell wrapper (if it exists)
    let shell_config_path = get_shell_config_path()?;
    if shell_config_exists_with_pattern(&shell_config_path, "### BEGIN Gemini CLI Wrapper")? {
        remove_shell_function(
            &shell_config_path,
            "### BEGIN Gemini CLI Wrapper",
            "### END Gemini CLI Wrapper",
        )?;
    }

    // 7. Remove Configuration Directory (if forced)
    if force {
        warn!(
            "Force specified. Removing configuration directory: {}",
            config_dir.display()
        );
        if config_dir.exists() {
            if let Err(e) = fs::remove_dir_all(config_dir) {
                error!(
                    "Failed to remove configuration directory {}: {}. Manual removal may be required.",
                    config_dir.display(),
                    e
                );
            } else {
                info!(
                    "Removed configuration directory: {}",
                    config_dir.display()
                );
            }
        } else {
            info!("Configuration directory not found, skipping removal.");
        }
    } else {
        info!(
            "Configuration directory {} not removed. Use --force to remove it.",
            config_dir.display()
        );
    }

    info!("Uninstallation completed.");
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
    info!("Starting update...");

    // 1. Stop running daemons (if updating them)
    let runtime_dir = get_runtime_dir()?;
    if update_daemons {
        info!("Stopping daemons before update...");
        for daemon in daemons {
            stop_daemon_if_running(daemon, &runtime_dir)?;
        }
    } else {
        info!("Skipping daemon stop/update.");
    }

    // 2. Build necessary binaries
    build_binaries(update_manager_tool)?;

    // 3. Install/Update CLI binary
    install_binary("gemini-cli", install_dir)?;

    // 4. Install/Update Daemon binaries (if requested)
    if update_daemons {
        for daemon in daemons {
            install_binary(&daemon.bin_name, install_dir)?;
        }
    } else {
        info!("Skipping installation of daemon binaries.");
    }

    // 5. Install/Update Manager Tool (if requested)
    if update_manager_tool {
        install_manager(install_dir)?;
    } else {
        info!("Skipping gemini-manager update.");
    }

    // 6. Copy python_mcp directory
    let python_mcp_source_path = PathBuf::from("python_mcp");
    let python_mcp_dest_path = install_dir.join("python_mcp");
    // Remove old version first
    if python_mcp_dest_path.is_dir() {
        info!("Removing old Python MCP server directory for update: {}", python_mcp_dest_path.display());
        if let Err(e) = fs::remove_dir_all(&python_mcp_dest_path) {
             error!("Failed to remove old {} during update: {}. Manual removal may be required.", python_mcp_dest_path.display(), e);
             // Decide whether to proceed or error out. Let's proceed but warn.
        }
    }
    // Copy new version
    if python_mcp_source_path.is_dir() {
        info!(
            "Copying updated Python MCP server implementation to {}...",
            python_mcp_dest_path.display()
        );
        copy_dir_all(&python_mcp_source_path, &python_mcp_dest_path)?;
    } else {
        warn!(
            "Source directory 'python_mcp' not found, skipping Python MCP server update."
        );
    }

    // 7. Create/Update Unified Configuration (Always run this to ensure paths are correct)
    install_unified_config(install_dir, config_dir, mcp_servers)?;

    // 8. Ensure install_dir is in PATH
    ensure_path_contains(install_dir)?;

    // 9. Ensure shell wrapper exists (create if missing)
    create_cli_wrapper(install_dir, config_dir)?;

    info!("Update completed successfully!");
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

/// Recursively copies a directory and its contents.
fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> Result<()> {
    let src_path = src.as_ref();
    let dst_path = dst.as_ref();
    fs::create_dir_all(&dst_path)?;
    debug!(
        "Recursively copying from {} to {}",
        src_path.display(),
        dst_path.display()
    );
    for entry_result in fs::read_dir(src_path)? {
        let entry = entry_result?;
        let file_type = entry.file_type()?;
        let dst_entry_path = dst_path.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_all(entry.path(), dst_entry_path)?;
        } else {
            fs::copy(entry.path(), &dst_entry_path)?;
            debug!(
                "Copied file {} to {}",
                entry.path().display(),
                dst_entry_path.display()
            );
        }
    }
    Ok(())
}
