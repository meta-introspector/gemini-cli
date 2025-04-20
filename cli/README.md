# Gemini Suite CLI (`gemini-cli`)

This crate provides the primary command-line interface (`gemini-cli-bin`) for interacting with the Google Gemini API, leveraging the functionalities of the other crates in the Gemini Suite (`gemini-core`, `gemini-mcp`, `gemini-memory`).

It allows users to chat with Gemini, execute single prompts, manage history, utilize tools via the Model Context Protocol (MCP), and leverage persistent memory for context-aware interactions.

## Features

*   **Interactive Chat Mode**: Provides a REPL-style interface for conversational interactions with Gemini.
*   **Single Prompt Execution**: Supports sending single prompts directly from the command line.
*   **Gemini API Interaction**: Uses `gemini-core` to communicate with the Gemini API (`generateContent`).
*   **Configuration Management**: Loads configuration (API key, model, system prompt, feature flags) from `~/.config/gemini-cli/config.toml` and environment variables (`GEMINI_API_KEY`). Allows setting configuration via flags (`--set-api-key`, etc.).
*   **MCP Integration**: 
    *   Connects to an MCP host (either the standalone `mcp-hostd` daemon via IPC or by initializing `gemini-mcp::McpHost` internally).
    *   Discovers available tools/resources from the MCP host.
    *   Includes discovered capabilities in the system prompt sent to Gemini.
    *   Translates Gemini function calls into MCP tool execution requests.
    *   Displays tool execution results.
*   **Memory Integration**:
    *   Uses `gemini-memory::MemoryStore` for persistent memory.
    *   Leverages the MCP connection (via `McpHostInterface`) to generate embeddings for memories.
    *   Automatically enhances user prompts with relevant context retrieved from the memory store (toggleable via config).
    *   Supports automatic storage of conversation turns into memory (toggleable via config).
*   **Chat History**: Maintains persistent chat history across sessions (toggleable via flag/config), storing logs in `~/.local/share/gemini-cli/history/`.
*   **Formatted Output**: Renders Gemini's responses with markdown formatting and syntax highlighting.
*   **Built-in Server Execution**: The `gemini-cli-bin` executable can also run the built-in MCP servers (`filesystem`, `command`, `memory-store`) directly using flags (`--filesystem-mcp`, `--command-mcp`, `--memory-store-mcp`).

## Installation

Build the binary using Cargo:

```bash
cargo build --release
# The binary will be located at target/release/gemini-cli-bin

# Optionally, copy it to a location in your PATH
# cp target/release/gemini-cli-bin ~/.local/bin/gemini
```

## Configuration

1.  **API Key**: The CLI requires a Google Gemini API key. Set it using one of the following methods (highest precedence first):
    *   Environment Variable: `export GEMINI_API_KEY="YOUR_API_KEY"`
    *   Configuration File: Run `gemini-cli-bin --set-api-key YOUR_API_KEY`. This saves it to `~/.config/gemini-cli/config.toml`.

2.  **Configuration File (`~/.config/gemini-cli/config.toml`)**: Stores settings like API key, model name, system prompt, and feature flags.
    ```toml
    api_key = "your_api_key_here" # Optional if using env var
    system_prompt = "You are a helpful AI assistant." # Optional, default provided
    model_name = "gemini-1.5-flash-latest" # Optional, default provided
    save_history = true # Optional, default: true
    enable_memory_broker = true # Optional, default: true (enables prompt enhancement)
    enable_auto_memory = true # Optional, default: true (enables storing conversation turns)
    # async_memory_enabled = true # Optional, default: true (for async memory processing)
    ```
    You can edit this file manually or use flags like `--set-system-prompt` and `--set-model`.

3.  **MCP Servers (`~/.config/gemini-cli/mcp_servers.json`)**: If *not* running the built-in servers directly via flags, configure external MCP servers for the `mcp-hostd` daemon or the internal host fallback. See the `gemini-mcp` README for the format.

## Usage

```bash
# Basic prompt
gemini-cli-bin "Explain the difference between TCP and UDP."

# Interactive chat mode (uses history by default)
gemini-cli-bin -i
# Inside chat: Type your message, press Enter. Use /exit or Ctrl+C to quit.

# Start a new chat session (ignores previous history for this session)
gemini-cli-bin -i --new-chat

# Disable history saving for this command
gemini-cli-bin --disable-history "Tell me a joke."

# --- MCP Tool Usage (Requires MCP Host/Servers running or configured) ---

# Ask Gemini to use a filesystem tool (assumes filesystem MCP server is available)
gemini-cli-bin "Read the first 5 lines of my Cargo.toml file in the current directory."

# Ask Gemini to use a command tool (assumes command MCP server is available)
gemini-cli-bin "List the files in the src directory."

# --- Running Built-in MCP Servers ---

# Run the filesystem server (listens on stdio)
gemini-cli-bin --filesystem-mcp

# Run the command server (listens on stdio)
gemini-cli-bin --command-mcp

# Run the memory store server (listens on stdio)
gemini-cli-bin --memory-store-mcp

# (These server flags are mutually exclusive with normal prompt/chat modes)

# --- Other Flags ---

# See all available flags
gemini-cli-bin --help
```

## Dependencies

This CLI relies on the following workspace crates:

*   `gemini-core`: For base Gemini API client, types, and configuration.
*   `gemini-mcp`: For MCP host logic, configuration, and communication.
*   `gemini-memory`: For persistent memory storage, semantic search, and prompt enhancement. 