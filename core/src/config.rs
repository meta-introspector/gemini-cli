use crate::errors::{GeminiError, GeminiResult};
use dirs;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};
use toml; // Explicitly import toml // Explicitly import dirs

/// Configuration for the Gemini API
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GeminiApiConfig {
    /// API key for the Gemini API
    pub api_key: Option<String>,

    /// System prompt to use for interactions
    pub system_prompt: Option<String>,

    /// Model name to use for primary interactions
    pub model_name: Option<String>,

    /// Whether to save history
    pub save_history: Option<bool>,

    /// Whether to enable the memory broker
    pub enable_memory_broker: Option<bool>,

    /// Whether to enable automatic memory storage
    pub enable_auto_memory: Option<bool>,

    /// Model to use for memory broker operations (typically smaller/faster than main model)
    pub memory_broker_model: Option<String>,
}

impl Default for GeminiApiConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            system_prompt: Some(
                "You are a helpful assistant. Answer the user's questions concisely and accurately."
                .to_string()
            ),
            model_name: Some("gemini-2.5-pro-preview-03-25".to_string()),
            save_history: Some(true),
            enable_memory_broker: Some(true),
            enable_auto_memory: Some(true),
            memory_broker_model: Some("gemini-2.0-flash".to_string()),
        }
    }
}

/// Transport mechanism for MCP servers
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum McpTransport {
    /// Standard input/output communication
    Stdio,
    /// Server-Sent Events over HTTP
    SSE {
        /// URL for the SSE connection
        url: String,
        /// Optional headers to include with requests
        headers: Option<HashMap<String, String>>,
    },
    /// WebSocket communication
    WebSocket {
        /// URL for the WebSocket connection
        url: String,
        /// Optional headers to include with requests
        headers: Option<HashMap<String, String>>,
    },
}

/// Configuration for an MCP server
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfig {
    /// Name of the server (used as an identifier)
    pub name: String,

    /// Whether the server is enabled
    pub enabled: bool,

    /// Transport mechanism to use (stdio, sse, websocket)
    pub transport: McpTransport,

    /// Command and arguments to run the server
    pub command: Vec<String>,

    /// Additional arguments to pass to the command
    #[serde(default)]
    pub args: Vec<String>,

    /// Environment variables to set when running the server
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Tools that should run without confirmation
    #[serde(default)]
    pub auto_execute: Vec<String>,
}

/// Represents the unified configuration for the entire Gemini Suite.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "kebab-case")]
pub struct UnifiedConfig {
    /// Configuration for the CLI
    #[serde(default)]
    pub cli: CliConfig,

    /// Configuration for the HAPPE daemon
    #[serde(default)]
    pub happe: HappeConfig,

    /// Configuration for the IDA daemon
    #[serde(default)]
    pub ida: IdaConfig,

    /// Configuration for memory operations
    #[serde(default)]
    pub memory: MemoryConfig,

    /// Configuration for MCP operations
    #[serde(default)]
    pub mcp: McpConfig,

    /// Configuration for the daemon manager
    #[serde(default)]
    pub daemon_manager: DaemonManagerConfig,

    /// Gemini API configuration (shared by multiple components)
    #[serde(default)]
    pub gemini_api: GeminiApiConfig,
}

/// Configuration specific to the CLI.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "kebab-case")]
pub struct CliConfig {
    /// Path to the history file
    pub history_file_path: Option<PathBuf>,

    /// Default log level
    pub log_level: Option<String>,

    /// Socket path for connecting to HAPPE
    pub happe_ipc_path: Option<PathBuf>,
}

/// Configuration specific to the HAPPE daemon.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "kebab-case")]
pub struct HappeConfig {
    /// Path to the IDA daemon socket
    pub ida_socket_path: Option<PathBuf>,

    /// Path to the HAPPE daemon socket
    pub happe_socket_path: Option<PathBuf>,

    /// Whether HTTP server is enabled
    pub http_enabled: Option<bool>,

    /// Bind address for HTTP server
    pub http_bind_addr: Option<String>,

    /// System prompt for HAPPE interactions
    pub system_prompt: Option<String>,
}

/// Memory broker LLM configuration
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "kebab-case")]
pub struct MemoryBrokerConfig {
    /// The provider for the broker LLM ("gemini", "openai", "ollama", "anthropic", etc.)
    pub provider: Option<String>,
    /// API key (if required by the provider)
    pub api_key: Option<String>,
    /// Model name/identifier for the provider
    pub model_name: Option<String>,
    /// Base URL (primarily for Ollama or other self-hosted endpoints)
    pub base_url: Option<String>,
}

/// Configuration specific to the IDA daemon.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "kebab-case")]
pub struct IdaConfig {
    /// Path to the IDA daemon socket
    pub ida_socket_path: Option<PathBuf>,

    /// Path to the memory database
    pub memory_db_path: Option<PathBuf>,

    /// Maximum number of memory results to return
    pub max_memory_results: Option<usize>,

    /// Semantic similarity threshold for memory retrieval
    pub semantic_similarity_threshold: Option<f32>,

    /// Memory broker configuration
    #[serde(default)]
    pub memory_broker: MemoryBrokerConfig,
}

/// Configuration specific to memory operations.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "kebab-case")]
pub struct MemoryConfig {
    /// Path to the LanceDB database
    pub db_path: Option<PathBuf>,

    /// Embedding model variant to use
    pub embedding_model_variant: Option<String>,

    /// Path to storage for embeddings/models
    pub storage_path: Option<String>,

    /// Embedding model to use (will be mapped to a variant)
    pub embedding_model: Option<String>,
}

/// Configuration specific to the MCP component.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "kebab-case")]
pub struct McpConfig {
    /// Path to the MCP servers JSON file
    pub mcp_servers_file_path: Option<PathBuf>,

    /// Path to the MCP host daemon socket
    pub mcp_host_socket_path: Option<PathBuf>,
}

/// Configuration specific to the Daemon Manager.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "kebab-case")]
pub struct DaemonManagerConfig {
    /// Where to install daemon executables
    pub daemon_install_path: Option<PathBuf>,

    /// Editor for config editing (from EDITOR env var)
    pub show_config_editor: Option<String>,
}

/// Claude-compatible server configuration for serializing to the JSON format.
#[derive(Serialize)]
struct ClaudeServer {
    command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    args: Vec<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    env: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    enabled: Option<bool>,
}

impl UnifiedConfig {
    /// Loads the unified configuration from the default location.
    /// This combines finding the path and loading from it.
    /// Returns default config if file doesn't exist or on error finding path/reading/parsing.
    pub fn load() -> Self {
        match get_unified_config_path() {
            Ok(config_path) => Self::load_from_file(&config_path).unwrap_or_else(|e| {
                eprintln!(
                    "Warning: Failed to load config from {}: {}. Using default.",
                    config_path.display(),
                    e
                );
                Self::default()
            }),
            Err(e) => {
                eprintln!(
                    "Warning: Failed to determine config path: {}. Using default.",
                    e
                );
                Self::default()
            }
        }
    }

    /// Loads configuration from a specific file path.
    /// Returns default config if file doesn't exist.
    pub fn load_from_file(path: &Path) -> GeminiResult<Self> {
        if path.exists() {
            let content = fs::read_to_string(path).map_err(|e| {
                GeminiError::ConfigError(format!(
                    "Failed to read unified config file '{}': {}",
                    path.display(),
                    e
                ))
            })?;

            let config: Self = toml::from_str(&content).map_err(|e| {
                GeminiError::ConfigError(format!(
                    "Failed to parse unified config file '{}': {}",
                    path.display(),
                    e
                ))
            })?;

            Ok(config)
        } else {
            // Indicate that the default is being used because the file is missing
            eprintln!(
                "Info: Config file not found at '{}'. Using default configuration.",
                path.display()
            );
            Ok(Self::default())
        }
    }

    /// Saves configuration to the default file path.
    pub fn save(&self) -> GeminiResult<()> {
        let path = get_unified_config_path()?;
        self.save_to_file(&path)
    }

    /// Saves configuration to a specific file path.
    pub fn save_to_file(&self, path: &Path) -> GeminiResult<()> {
        let content = toml::to_string_pretty(self).map_err(|e| {
            // Use pretty print
            GeminiError::ConfigError(format!("Failed to serialize unified config: {}", e))
        })?;

        // Ensure the directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                GeminiError::ConfigError(format!(
                    "Failed to create config directory '{}': {}",
                    parent.display(),
                    e
                ))
            })?;
        }

        fs::write(path, content).map_err(|e| {
            GeminiError::ConfigError(format!(
                "Failed to write unified config file '{}': {}",
                path.display(),
                e
            ))
        })?;
        tracing::info!("Configuration saved to '{}'", path.display());
        Ok(())
    }

    /// Creates a new unified configuration with default values plus any overrides
    pub fn new_with_defaults(
        cli: Option<CliConfig>,
        happe: Option<HappeConfig>,
        ida: Option<IdaConfig>,
        memory: Option<MemoryConfig>,
        mcp: Option<McpConfig>,
        daemon_manager: Option<DaemonManagerConfig>,
        gemini_api: Option<GeminiApiConfig>,
    ) -> Self {
        Self {
            cli: cli.unwrap_or_default(),
            happe: happe.unwrap_or_default(),
            ida: ida.unwrap_or_default(),
            memory: memory.unwrap_or_default(),
            mcp: mcp.unwrap_or_default(),
            daemon_manager: daemon_manager.unwrap_or_default(),
            gemini_api: gemini_api.unwrap_or_default(),
        }
    }
}

/// Gets the path to the unified configuration file.
/// Checks `GEMINI_SUITE_CONFIG_PATH` env var first, then defaults to ~/.config/gemini-suite/config.toml.
pub fn get_unified_config_path() -> GeminiResult<PathBuf> {
    // Check for GEMINI_SUITE_CONFIG_PATH environment variable first
    if let Ok(env_path_str) = std::env::var("GEMINI_SUITE_CONFIG_PATH") {
        if !env_path_str.is_empty() {
            let path = PathBuf::from(&env_path_str);
            // Simple check: does it look like a file path (has a filename component)?
            if path.file_name().is_some() {
                // Log the path for debugging
                // Use tracing instead of eprintln
                // tracing::debug!("Using config path from GEMINI_SUITE_CONFIG_PATH: {}", path.display());
                return Ok(path);
            } else {
                // Use tracing instead of eprintln
                tracing::warn!("GEMINI_SUITE_CONFIG_PATH ('{}') does not look like a valid file path. Falling back to default.", env_path_str);
            }
        }
    }
    
    // Otherwise use the default location: ~/.config/gemini-suite/config.toml
    let config_dir = dirs::config_dir()
        .ok_or_else(|| {
            GeminiError::ConfigError("Could not determine user config directory".to_string())
        })?
        .join("gemini-suite");

    let config_path = config_dir.join("config.toml");

    // Log the path for debugging
    // Use tracing instead of eprintln
    // tracing::debug!("Using default config path: {}", config_path.display());

    Ok(config_path)
}

/// Gets the default path for MCP servers configuration file.
/// This builds on the unified config path logic but returns a different file.
pub fn get_mcp_servers_config_path() -> GeminiResult<PathBuf> {
    let config_path = get_unified_config_path()?;
    let parent = config_path.parent().ok_or_else(|| {
        GeminiError::ConfigError("Could not determine parent directory of config path".to_string())
    })?;

    Ok(parent.join("mcp_servers.json"))
}

/// Loads MCP server configurations from the default location or a specific path.
pub fn load_mcp_servers(config_path: Option<&Path>) -> GeminiResult<Vec<McpServerConfig>> {
    let path = if let Some(p) = config_path {
        p.to_path_buf()
    } else {
        get_mcp_servers_config_path()?
    };

    if !path.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(&path).map_err(|e| {
        GeminiError::ConfigError(format!(
            "Failed to read MCP servers config file '{}': {}",
            path.display(),
            e
        ))
    })?;

    if content.trim().is_empty() {
        return Ok(Vec::new());
    }

    // Try parsing as a Vec<McpServerConfig> first (older format)
    let servers_vec_result: Result<Vec<McpServerConfig>, _> = serde_json::from_str(&content);

    if let Ok(servers) = servers_vec_result {
        return Ok(servers);
    }

    // If that fails, try parsing as an object with a "servers" field (newer format)
    #[derive(serde::Deserialize)]
    struct ServersContainer {
        servers: Vec<McpServerConfig>,
    }

    let servers_container_result: Result<ServersContainer, _> = serde_json::from_str(&content);

    if let Ok(container) = servers_container_result {
        return Ok(container.servers);
    }

    // If that fails too, try parsing as the Claude-compatible format
    #[derive(serde::Deserialize)]
    struct ClaudeServerConfig {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: HashMap<String, String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        enabled: Option<bool>,
    }

    #[derive(serde::Deserialize)]
    struct ClaudeServersConfig {
        #[serde(rename = "mcpServers")]
        mcp_servers: HashMap<String, ClaudeServerConfig>,
    }

    let claude_config_result: Result<ClaudeServersConfig, _> = serde_json::from_str(&content);

    if let Ok(claude_config) = claude_config_result {
        let mut servers = Vec::new();

        for (name, server) in claude_config.mcp_servers {
            servers.push(McpServerConfig {
                name,
                enabled: server.enabled.unwrap_or(true),
                transport: McpTransport::Stdio,
                command: vec![server.command],
                args: server.args,
                env: server.env,
                auto_execute: Vec::new(),
            });
        }

        return Ok(servers);
    }

    // All parsing attempts failed
    Err(GeminiError::ConfigError(format!(
        "Failed to parse MCP servers config file '{}': invalid format",
        path.display()
    )))
}

/// Save MCP server configurations to a file in the Claude-compatible format.
pub fn save_mcp_servers(
    servers: &[McpServerConfig],
    custom_path: Option<&Path>,
) -> GeminiResult<()> {
    let path = if let Some(p) = custom_path {
        p.to_path_buf()
    } else {
        get_mcp_servers_config_path()?
    };

    // Convert to Claude-compatible format
    let mut claude_servers = HashMap::new();

    for server in servers {
        let command = server.command.first().cloned().unwrap_or_default();

        claude_servers.insert(
            server.name.clone(),
            ClaudeServer {
                command,
                args: server.args.clone(),
                env: server.env.clone(),
                enabled: Some(server.enabled),
            },
        );
    }

    #[derive(Serialize)]
    struct ClaudeServersConfig {
        #[serde(rename = "mcpServers")]
        mcp_servers: HashMap<String, ClaudeServer>,
    }

    let claude_config = ClaudeServersConfig {
        mcp_servers: claude_servers,
    };

    // Serialize in the Claude-compatible format
    let content = serde_json::to_string_pretty(&claude_config).map_err(|e| {
        GeminiError::ConfigError(format!("Failed to serialize MCP servers config: {}", e))
    })?;

    // Ensure the directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            GeminiError::ConfigError(format!(
                "Failed to create directory '{}': {}",
                parent.display(),
                e
            ))
        })?;
    }

    fs::write(&path, content).map_err(|e| {
        GeminiError::ConfigError(format!(
            "Failed to write MCP servers config to '{}': {}",
            path.display(),
            e
        ))
    })?;

    Ok(())
}
