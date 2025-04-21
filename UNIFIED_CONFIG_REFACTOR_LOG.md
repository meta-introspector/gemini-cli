# Unified Configuration Refactoring Log

This file tracks the identification and refactoring of configuration logic across Gemini Suite crates. 

## CLI Crate (`cli`)

### Configuration Mechanisms Found

| File                | Line(s)   | Type                  | Description                                               |
|---------------------|-----------|------------------------|----------------------------------------------------------|
| `cli/src/main.rs`   | 26        | Environment Loading    | `dotenv().ok();` - Loads `.env` files                     |
| `cli/src/main.rs`   | 29        | Command-line Args      | `Args::parse()` - Parses command-line arguments           |
| `cli/src/main.rs`   | 24        | Hardcoded Default      | Log level defaults to `info`                              |
| `cli/src/cli.rs`    | 17        | Environment Variable   | `happe_ipc_path` can be set via `HAPPE_IPC_PATH` env var  |
| `cli/src/cli.rs`    | 14        | Hardcoded Default      | `interactive` defaults to `false`                         |
| `cli/src/cli.rs`    | 21,25,29  | Hardcoded Default      | MCP server flags default to `false`                       |

### Refactoring Strategy

1. Add `gemini-core` dependency to `cli/Cargo.toml`.
2. Modify `cli/src/main.rs` to:
   - Load the unified configuration first using `gemini_core::config::UnifiedConfig::load()`.
   - Keep command-line arguments parsing but prioritize them over config file values.
   - Modify `HappeClient::new()` call to use `happe_ipc_path` from unified config if not provided via command line.
   - Consider making log level configurable via unified config if not overridden by command line.

3. The priority order should be:
   - Command-line arguments (highest precedence)
   - Environment variables (transitional support)
   - Unified configuration file
   - Hardcoded defaults (lowest precedence)

## Daemon Manager Crate (`daemon-manager`)

### Configuration Mechanisms Found

| File                         | Line(s)   | Type                  | Description                                                      |
|------------------------------|-----------|------------------------|------------------------------------------------------------------|
| `daemon-manager/src/config.rs` | 8-19    | Hardcoded Defaults     | `DEFAULT_MCP_SERVERS_CONFIG` and `DEFAULT_GEMINI_CONFIG` constants |
| `daemon-manager/src/config.rs` | 22-59   | File Path Logic        | `get_config_path()` determines config file locations              |
| `daemon-manager/src/config.rs` | 62-86   | File Path Logic        | `get_old_config_path()` for backward compatibility               |
| `daemon-manager/src/config.rs` | 89-101  | Hardcoded Defaults     | `get_default_config()` for various components                    |
| `daemon-manager/src/config.rs` | 104-115 | File Operations        | `show_config()` reads and displays config                        |
| `daemon-manager/src/config.rs` | 118-144 | File Operations        | `edit_config()` edits component config files                     |
| `daemon-manager/src/config.rs` | 147-166 | File Operations        | `reset_config()` resets config files to defaults                 |
| `daemon-manager/src/mcp.rs`    | 20-113  | Data Structures        | MCP server configuration structures                              |
| `daemon-manager/src/mcp.rs`    | 116-139 | File Path Logic        | `get_mcp_config_path()` for MCP server config                    |
| `daemon-manager/src/mcp.rs`    | 142-206 | File Operations        | `read_mcp_config()` reads config in multiple formats             |
| `daemon-manager/src/mcp.rs`    | 209-259 | File Operations        | `write_mcp_config()` writes in Claude-compatible format          |

### Refactoring Strategy

1. Add `gemini-core` dependency to `daemon-manager/Cargo.toml`.
2. Create `DaemonManagerConfig` struct in `core/src/config.rs` with:
   - Fields for paths and settings currently hardcoded or determined at runtime
   - Settings for the daemon manager's behavior

3. Update the `McpConfig` in `core/src/config.rs` to:
   - Support representing MCP servers configuration
   - Include a field for `mcp_servers_file_path` indicating where the MCP servers JSON file is stored
   - Note: This assumes we still keep MCP servers in a separate JSON file (as mentioned "except the mcp_servers.json file")

4. Modify `daemon-manager/src/config.rs` and `daemon-manager/src/mcp.rs` to:
   - Load settings from `UnifiedConfig::load()`
   - Remove file path determination logic, using paths from unified config
   - Keep actual file operations but sourced from paths in unified config
   - Use unified config for defaults rather than hardcoded constants

5. Update daemon manager commands that manipulate configs to work with the unified config approach

## HAPPE Daemon Crate (`happe`)

### Configuration Mechanisms Found

| File                      | Line(s)   | Type                  | Description                                                     |
|---------------------------|-----------|------------------------|-----------------------------------------------------------------|
| `happe/src/bin/happe-daemon.rs` | 13-45 | Command-line Args   | `Args` struct with command-line options                         |
| `happe/src/bin/happe-daemon.rs` | 59-93 | Config Loading      | Logic to load config from file or defaults, then override with CLI args |
| `happe/src/config.rs`     | 8-34      | Data Structure        | `AppConfig` struct definition                                   |
| `happe/src/config.rs`     | 36-54     | Hardcoded Defaults    | `Default` implementation for `AppConfig`                        |
| `happe/src/config.rs`     | 57-81     | File Loading          | `load_from_file()` loads config from a file                     |
| `happe/src/config.rs`     | 83-98     | Path Logic            | `get_config_dir()` determines config directory                  |
| `happe/src/config.rs`     | 100-112   | Config Loading        | `load_from_default()` tries unified config or falls back to defaults |
| `happe/src/config.rs`     | 114-168   | Config Conversion     | `load_from_unified_config()` pulls HAPPE settings from `UnifiedConfig` |
| `happe/src/config.rs`     | 170-191   | MCP Config Loading    | `load_mcp_config()` loads MCP server configuration              |

### Refactoring Strategy

1. The HAPPE daemon already has initial support for the unified config structure, as it imports and uses
   `gemini_core::config::{GeminiConfig, UnifiedConfig, get_unified_config_path}` and has methods like
   `load_from_unified_config()`. However, the implementation still has:
   
   - Parallel logic for loading from a standalone config file
   - A combination of direct socket path configuration and reading from `UnifiedConfig.happe`
   - Command-line overrides that duplicate the unified config schema
   - Legacy loading of MCP servers

2. Refactoring should:
   - Update the `HappeConfig` struct in `core/src/config.rs` to include all fields from `AppConfig`
   - Simplify `happe/src/config.rs` to use the unified config structure directly
   - Retain command-line argument overrides but ensure they match the unified config fields
   - Remove the standalone config file loading path, only supporting unified config (with CLI overrides)
   - Remove `get_config_dir()` in favor of the unified config path logic in `core/src/config.rs`
   - Update MCP integration via a unified MCP server file path from the config

## IDA Daemon Crate (`ida`)

### Configuration Mechanisms Found

| File                      | Line(s)   | Type                  | Description                                                     |
|---------------------------|-----------|------------------------|-----------------------------------------------------------------|
| `ida/src/bin/ida-daemon.rs` | 8-27   | Command-line Args     | `Args` struct with command-line options                         |
| `ida/src/bin/ida-daemon.rs` | 29-38  | Logging Setup         | Custom log level parsing from CLI args                           |
| `ida/src/bin/ida-daemon.rs` | 42-78  | Config Loading        | Logic to load config or fallback to defaults, then override with CLI args |
| `ida/src/config.rs`       | 8-27     | Data Structure        | `IdaConfig` struct definition                                   |
| `ida/src/config.rs`       | 29-37    | Data Structure        | `MemoryBrokerConfig` struct for memory broker settings          |
| `ida/src/config.rs`       | 39-51    | Hardcoded Defaults    | `Default` implementation for `IdaConfig`                        |
| `ida/src/config.rs`       | 54-75    | File Loading          | `load_from_file()` loads config from a file                     |
| `ida/src/config.rs`       | 77-93    | Path Logic            | `get_config_dir()` determines config directory                  |
| `ida/src/config.rs`       | 95-102   | Config Loading        | `load_from_default()` tries unified config or falls back to defaults |
| `ida/src/config.rs`       | 104-168  | Config Conversion     | `load_from_unified_config()` attempts to extract IDA settings from unified config |
| `ida/src/config.rs`       | 170-198  | Path Resolution       | `resolve_memory_db_path()` converts relative paths to absolute  |

### Refactoring Strategy

1. The IDA daemon also has partial support for the unified config structure, as it imports `get_unified_config_path` and has methods like `load_from_unified_config()`. However, the implementation has issues:
   
   - The current `load_from_unified_config` uses manual TOML manipulation instead of directly reading from a properly defined `UnifiedConfig` structure
   - Still has parallel logic for loading from a standalone config file
   - Maintains its own path resolution logic in `resolve_memory_db_path`
   - Has its own config directory determination in `get_config_dir`

2. Refactoring should:
   - Update the `IdaConfig` struct in `core/src/config.rs` to match the IDA daemon's config needs
   - Modify `ida/src/config.rs` to use the unified config directly rather than trying to manually parse it
   - Retain command-line argument overrides but ensure they match the unified config fields
   - Remove the redundant path resolution and config directory logic
   - Remove the legacy config file loading, focusing only on the unified config

## Memory Crate (`memory`)

### Configuration Mechanisms Found

| File                      | Line(s)   | Type                  | Description                                                     |
|---------------------------|-----------|------------------------|-----------------------------------------------------------------|
| `memory/src/config.rs`    | 5-12      | Path Logic            | `get_memory_db_path()` determines LanceDB storage location      |
| `memory/src/config.rs`    | 14-29     | Dir Creation          | `ensure_memory_db_dir()` creates necessary directories          |
| `memory/src/store.rs`     | 21, 58-60 | Config Usage          | `MemoryStore` uses config functions to determine DB path        |

### Refactoring Strategy

1. The memory crate has very minimal configuration logic, just focused on determining and creating the memory database path. The approach should be:

   - Update the `MemoryConfig` struct in `core/src/config.rs` to include:
     - `db_path`: Path to the LanceDB database directory 
     - `embedding_model_variant`: Default embedding model type/variant to use
     - Any other memory-related settings

   - Refactor `memory/src/config.rs` to:
     - Import and use `gemini_core::config::UnifiedConfig`
     - Replace `get_memory_db_path()` with a function that reads from the unified config
     - Keep `ensure_memory_db_dir()` as a utility but make it use the path from unified config
     
   - Update `memory/src/store.rs` to:
     - Take a `MemoryConfig` from the unified config in its constructor
     - Use the settings from this config instead of hardcoded values

## MCP Crate (`mcp`)

### Configuration Mechanisms Found

| File                      | Line(s)   | Type                  | Description                                                     |
|---------------------------|-----------|------------------------|-----------------------------------------------------------------|
| `mcp/src/config.rs`       | 8-26      | Data Structure        | `McpServerConfig` struct for MCP server definitions             |
| `mcp/src/config.rs`       | 28-36     | Data Structure        | `McpTransport` enum for transport types                         |
| `mcp/src/config.rs`       | 38-43     | Path Logic            | `get_config_dir()` determines config directory                  |
| `mcp/src/config.rs`       | 45-49     | Path Logic            | `get_mcp_config_path()` finds MCP servers file                  |
| `mcp/src/config.rs`       | 51-104    | File Loading          | `load_mcp_servers()` loads server configs from file             |
| `mcp/src/host/mod.rs`     | 569-606   | Config Writing        | Code to update MCP server configs in file                       |
| `mcp/src/host/mod.rs`     | 31-109    | Server Initialization | Creates servers from configs, special handing for different transports |
| `mcp/src/host/types.rs`   | 62-91     | Server Creation       | Creates a process-based server from config                      |

### Refactoring Strategy

1. The MCP crate has its own configuration loading logic that partially overlaps with the daemon-manager's logic for MCP servers. The unified approach should be:

   - Move the `McpServerConfig` and `McpTransport` definitions to `core/src/config.rs` since they're reused across multiple crates
   - Ensure the `McpConfig` struct in `core/src/config.rs` provides:
     - Path to the MCP servers JSON file (`mcp_servers_file_path`)
     - Path to the MCP host daemon socket (`mcp_host_socket_path`)
     - Any other MCP-related settings

   - Refactor `mcp/src/config.rs` to:
     - Import configuration types from `gemini_core::config`
     - Use `gemini_core::config::get_unified_config_path()` instead of having its own path logic
     - Replace `get_config_dir()` and `get_mcp_config_path()` with functions that read from the unified config
     - Update `load_mcp_servers()` to use the path from unified config
   
   - Update `mcp/src/host/mod.rs` and `mcp/src/host/types.rs` to:
     - Use the path from the unified config when saving server configurations
     - Keep the server initialization logic since it's operational in nature

## Summary of Required Changes to `core/src/config.rs`

Based on the audit findings, here's a summary of the changes needed to the `core/src/config.rs` file:

### 1. Shared Structures (Used by Multiple Crates)

- `McpServerConfig` and `McpTransport` (currently in `mcp/src/config.rs`):
  - Move to `core/src/config.rs`
  - Used by both `mcp` and `daemon-manager` crates

- `GeminiApiConfig` (from current `GeminiConfig`):
  - Fields: `api_key`, `model_name`, `system_prompt`, etc.
  - Used by most crates

### 2. Component-Specific Structs

Update all component config structs to include the full range of settings needed:

- **`CliConfig`**:
  - `history_file_path`: Option<PathBuf> - Path to the history file
  - `log_level`: Option<String> - Default log level
  - `happe_ipc_path`: Option<PathBuf> - Socket path for connecting to HAPPE

- **`HappeConfig`**:
  - `ida_socket_path`: PathBuf - Path to the IDA daemon socket
  - `happe_socket_path`: PathBuf - Path to the HAPPE daemon socket
  - `http_enabled`: bool - Whether HTTP server is enabled
  - `http_bind_addr`: String - Bind address for HTTP server
  - `system_prompt`: String - System prompt for Gemini interactions

- **`IdaConfig`**:
  - `ida_socket_path`: PathBuf - Path to the IDA daemon socket
  - `memory_db_path`: PathBuf - Path to the memory database
  - `max_memory_results`: usize - Maximum number of memory results to return
  - `semantic_similarity_threshold`: f32 - Threshold for memory similarity matching
  - `memory_broker`: MemoryBrokerConfig - Memory broker LLM settings

- **`MemoryConfig`**:
  - `db_path`: PathBuf - Path to the LanceDB database
  - `embedding_model_variant`: String - Default embedding model to use

- **`McpConfig`**:
  - `mcp_servers_file_path`: PathBuf - Path to the MCP servers JSON file
  - `mcp_host_socket_path`: PathBuf - Path to the MCP host daemon socket

- **`DaemonManagerConfig`**:
  - `daemon_install_path`: PathBuf - Where to install daemon executables
  - `show_config_editor`: String - Editor for config editing (from EDITOR env var)

### 3. Implementation Changes

- Ensure the `load()` method properly handles deserialization and defaults
- Ensure `get_unified_config_path()` uses a consistent logic for all components
- Ensure `save()` and `save_to_file()` functions can update the unified config 