// This crate will contain MCP tool calling client logic:
// - MCP client
// - Tool dispatch functionality
// - Function call handling

pub mod config;
pub mod gemini;
pub mod host;
pub mod ipc;
pub mod rpc;
pub mod servers;

// Re-export main types and functions for convenience
pub use host::McpHost;
// Re-export gemini types and functions
pub use gemini::{FunctionCall, FunctionDef, FunctionParameter,
               convert_mcp_tools_to_gemini_functions, build_mcp_system_prompt,
               sanitize_json_schema, parse_function_calls, generate_gemini_function_declarations,
               process_function_call};
// Remove re-export of types now in core
// pub use rpc::{ServerCapabilities, Tool, Resource};
pub use config::{get_mcp_config_path, load_mcp_servers, McpServerConfig, McpTransport};

// Re-export server modules
pub use servers::filesystem;
pub use servers::command;
pub use servers::memory_store;

// Modules will be added in Phase 4
