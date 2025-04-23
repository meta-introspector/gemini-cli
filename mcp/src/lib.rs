// This crate will contain MCP tool calling client logic:
// - MCP client
// - Tool dispatch functionality
// - Function call handling

pub mod config;
pub mod gemini;
pub mod host;
// pub mod ipc; // Removed, now handled by the dedicated `ipc` crate
pub mod rpc;

// Re-export main types and functions for convenience
pub use host::McpHost;
// Re-export gemini types and functions
pub use gemini::{
    build_mcp_system_prompt, convert_mcp_tools_to_gemini_functions,
    generate_gemini_function_declarations, parse_function_calls, process_function_call,
    sanitize_json_schema, FunctionCall, FunctionDef, FunctionParameter,
};
// Remove re-export of types now in core
// pub use rpc::{ServerCapabilities, Tool, Resource};
pub use config::{get_mcp_config_path, load_mcp_servers, McpServerConfig, McpTransport};


// Modules will be added in Phase 4
