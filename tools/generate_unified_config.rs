use gemini_core::config::{
    get_unified_config_path, DaemonManagerConfig, GeminiApiConfig, HappeConfig, IdaConfig,
    McpConfig, McpServerConfig, McpTransport, MemoryConfig, UnifiedConfig,
};
use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use toml;

fn main() -> Result<(), Box<dyn Error>> {
    println!("Generating unified configuration file for Gemini Suite...");

    // Get the unified config path using the core function
    let unified_config_path = get_unified_config_path()?;
    let config_dir = unified_config_path.parent().unwrap_or(Path::new("."));

    // Ensure the config directory exists
    fs::create_dir_all(config_dir)?;

    // Create a unified configuration with sensible defaults
    let mut unified_config = UnifiedConfig::default();

    // Set up CLI config with defaults
    unified_config.cli.log_level = Some("info".to_string());
    unified_config.cli.happe_ipc_path = Some(PathBuf::from("/tmp/gemini_suite_happe.sock"));
    unified_config.cli.history_file_path = Some(
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".config/gemini-suite/history.json"),
    );

    // Set up HAPPE config with defaults
    if unified_config.happe.ida_socket_path.is_none() {
        unified_config.happe.ida_socket_path = Some(PathBuf::from("/tmp/gemini_suite_ida.sock"));
    }
    if unified_config.happe.happe_socket_path.is_none() {
        unified_config.happe.happe_socket_path =
            Some(PathBuf::from("/tmp/gemini_suite_happe.sock"));
    }
    if unified_config.happe.http_enabled.is_none() {
        unified_config.happe.http_enabled = Some(true);
    }
    if unified_config.happe.http_bind_addr.is_none() {
        unified_config.happe.http_bind_addr = Some("127.0.0.1:8080".to_string());
    }

    // Set up IDA config with defaults
    if unified_config.ida.ida_socket_path.is_none() {
        unified_config.ida.ida_socket_path = Some(PathBuf::from("/tmp/gemini_suite_ida.sock"));
    }
    if unified_config.ida.memory_db_path.is_none() {
        unified_config.ida.memory_db_path = Some(config_dir.join("memory/lancedb"));
    }
    if unified_config.ida.max_memory_results.is_none() {
        unified_config.ida.max_memory_results = Some(10);
    }
    if unified_config.ida.semantic_similarity_threshold.is_none() {
        unified_config.ida.semantic_similarity_threshold = Some(0.7);
    }

    // Set up IDA Memory Broker config defaults
    if unified_config.ida.memory_broker.provider.is_none() {
        unified_config.ida.memory_broker.provider = Some("gemini".to_string()); // Default to gemini
    }
    // api_key defaults to None implicitly or via struct default
    if unified_config.ida.memory_broker.model_name.is_none() {
        unified_config.ida.memory_broker.model_name = Some("gemini-2.0-flash".to_string());
    }
    // base_url defaults to None implicitly or via struct default

    // Set up Memory config with defaults
    if unified_config.memory.db_path.is_none() {
        unified_config.memory.db_path = Some(config_dir.join("memory/lancedb"));
    }
    if unified_config.memory.embedding_model_variant.is_none() {
        unified_config.memory.embedding_model_variant = Some("base".to_string());
    }
    if unified_config.memory.embedding_model.is_none() {
        unified_config.memory.embedding_model = Some("gemini-2.0-flash".to_string());
    }

    // Set up MCP config with defaults
    if unified_config.mcp.mcp_servers_file_path.is_none() {
        unified_config.mcp.mcp_servers_file_path = Some(config_dir.join("mcp_servers.json"));
    }
    if unified_config.mcp.mcp_host_socket_path.is_none() {
        unified_config.mcp.mcp_host_socket_path = Some(PathBuf::from("/tmp/gemini_suite_mcp.sock"));
    }

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

        // Create standard MCP server configurations
        let filesystem_server = McpServerConfig {
            name: "filesystem".to_string(),
            enabled: true,
            transport: McpTransport::Stdio,
            command: vec!["builtin".to_string()],
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
            command: vec!["builtin".to_string()],
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
            command: vec!["builtin".to_string()],
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
        "Unified configuration file created at {:?}",
        unified_config_path
    );
    println!("The MCP servers configuration is at {:?}", mcp_servers_path);
    println!("You may need to modify them according to your needs.");

    Ok(())
}
