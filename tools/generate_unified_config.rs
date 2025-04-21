use gemini_core::config::{
    get_unified_config_path, McpServerConfig, McpTransport, UnifiedConfig,
};
use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    println!("Generating unified configuration file for Gemini Suite...");

    // Get the unified config path using the core function
    let unified_config_path = get_unified_config_path()?;
    let config_dir = unified_config_path.parent().unwrap_or(Path::new("."));

    // Ensure the config directory exists
    fs::create_dir_all(config_dir)?;

    // Create a unified configuration with sensible defaults
    let mut unified_config = UnifiedConfig::default();

    // --- Set up [gemini-api] defaults --- 
    // Ensure api_key is None by default, other fields use defaults from GeminiApiConfig::default()
    unified_config.gemini_api.api_key = None; 
    // The rest like model_name, system_prompt, etc., inherit from `default()`
    // --- ---

    // Set up CLI config with defaults
    unified_config.cli.log_level = Some("info".to_string());
    // Default socket paths are now resolved at runtime if not set, so remove them here
    // unified_config.cli.happe_ipc_path = Some(PathBuf::from("/tmp/gemini_suite_happe.sock")); 
    unified_config.cli.history_file_path = Some(
        config_dir.join("history.json") // Place history inside config dir by default
    );

    // Set up HAPPE config with defaults
    // Default socket paths are resolved at runtime if not set
    // unified_config.happe.ida_socket_path = Some(PathBuf::from("/tmp/gemini_suite_ida.sock")); 
    // unified_config.happe.happe_socket_path = Some(PathBuf::from("/tmp/gemini_suite_happe.sock")); 
    if unified_config.happe.http_enabled.is_none() {
        unified_config.happe.http_enabled = Some(true);
    }
    if unified_config.happe.http_bind_addr.is_none() {
        unified_config.happe.http_bind_addr = Some("127.0.0.1:8080".to_string());
    }
    // system_prompt will inherit from [gemini-api] if not overridden here

    // Set up IDA config with defaults
    // unified_config.ida.ida_socket_path = Some(PathBuf::from("/tmp/gemini_suite_ida.sock")); // Resolved at runtime
    if unified_config.ida.memory_db_path.is_none() {
        unified_config.ida.memory_db_path = Some(config_dir.join("memory/db")); // Use /db suffix
    }
    if unified_config.ida.max_memory_results.is_none() {
        unified_config.ida.max_memory_results = Some(10);
    }
    if unified_config.ida.semantic_similarity_threshold.is_none() {
        unified_config.ida.semantic_similarity_threshold = Some(0.7);
    }

    // Set up IDA Memory Broker config defaults
    // provider defaults to None implicitly
    // api_key defaults to None implicitly
    if unified_config.ida.memory_broker.model_name.is_none() {
        // Inherit from gemini_api default if not set
        unified_config.ida.memory_broker.model_name = unified_config.gemini_api.memory_broker_model.clone(); 
    }
    // base_url defaults to None implicitly

    // Set up Memory config with defaults
    if unified_config.memory.db_path.is_none() {
        unified_config.memory.db_path = Some(config_dir.join("memory/db")); // Use /db suffix
    }
    if unified_config.memory.embedding_model_variant.is_none() {
        unified_config.memory.embedding_model_variant = Some("base".to_string());
    }
    if unified_config.memory.embedding_model.is_none() {
        unified_config.memory.embedding_model = Some("e5-small-v2".to_string()); // Use a specific default model
    }
    if unified_config.memory.storage_path.is_none() {
         unified_config.memory.storage_path = Some(config_dir.join("memory/models").to_string_lossy().to_string()); // Models path
    }

    // Set up MCP config with defaults
    if unified_config.mcp.mcp_servers_file_path.is_none() {
        unified_config.mcp.mcp_servers_file_path = Some(config_dir.join("mcp_servers.json"));
    }
    // unified_config.mcp.mcp_host_socket_path = Some(PathBuf::from("/tmp/gemini_suite_mcp.sock")); // Resolved at runtime

    // Set up Daemon Manager config with defaults
    if unified_config.daemon_manager.daemon_install_path.is_none() {
        unified_config.daemon_manager.daemon_install_path = Some(
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".local/bin"),
        );
    }
    if unified_config.daemon_manager.show_config_editor.is_none() {
        unified_config.daemon_manager.show_config_editor = std::env::var("EDITOR").ok();
    }

    // Generate a default MCP servers configuration if not already present
    let mcp_servers_path = unified_config
        .mcp
        .mcp_servers_file_path
        .clone()
        .unwrap_or_else(|| config_dir.join("mcp_servers.json"));

    if !mcp_servers_path.exists() {
        println!(
            "Generating default MCP servers configuration at {:?}",
            mcp_servers_path
        );

        // Define built-in server commands (use placeholders or actual paths if known)
        let install_path = unified_config.daemon_manager.daemon_install_path.clone().unwrap_or_default();
        let fs_cmd = install_path.join("filesystem-mcp").to_string_lossy().to_string();
        let cmd_cmd = install_path.join("command-mcp").to_string_lossy().to_string();
        let mem_cmd = install_path.join("memory-store-mcp").to_string_lossy().to_string();

        // Create standard MCP server configurations using Claude-compatible format
        let filesystem_server = McpServerConfig {
            name: "filesystem".to_string(),
            enabled: true,
            transport: McpTransport::Stdio,
            command: vec![fs_cmd], // Use resolved path
            args: vec![],
            env: {
                let mut env = HashMap::new();
                env.insert("GEMINI_MCP_TIMEOUT".to_string(), "120".to_string());
                env
            },
            auto_execute: vec![],
        };

        let command_server = McpServerConfig {
            name: "command".to_string(),
            enabled: true,
            transport: McpTransport::Stdio,
            command: vec![cmd_cmd], // Use resolved path
            args: vec![],
            env: {
                let mut env = HashMap::new();
                env.insert("GEMINI_MCP_TIMEOUT".to_string(), "120".to_string());
                env
            },
            auto_execute: vec![],
        };

        let memory_store_server = McpServerConfig {
            name: "memory-store".to_string(),
            enabled: true,
            transport: McpTransport::Stdio,
            command: vec![mem_cmd], // Use resolved path
            args: vec![],
            env: {
                let mut env = HashMap::new();
                env.insert("GEMINI_MCP_TIMEOUT".to_string(), "120".to_string());
                env
            },
            auto_execute: vec![
                "store_memory".to_string(),
                "list_all_memories".to_string(),
                "retrieve_memory_by_key".to_string(),
                "retrieve_memory_by_tag".to_string(),
                "delete_memory_by_key".to_string(),
            ],
        };

        let servers = vec![filesystem_server, command_server, memory_store_server];
        gemini_core::config::save_mcp_servers(&servers, Some(&mcp_servers_path))?;
    }

    // Serialize and write the unified config
    unified_config.save_to_file(&unified_config_path)?;

    println!(
        "Unified configuration file created/updated at {:?}",
        unified_config_path
    );
    println!("The MCP servers configuration is at {:?}", mcp_servers_path);
    println!("You may need to modify them according to your needs, especially the API key.");

    Ok(())
}
