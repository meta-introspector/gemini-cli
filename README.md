# Gemini Rust Suite 🦀

This project provides a suite of Rust crates for interacting with Google Gemini models, enabling advanced features like tool usage via the Model Context Protocol (MCP), persistent semantic memory, and a powerful command-line interface (CLI) for Linux terminals.

## ✨ Table of Contents

*   [Architecture](#-architecture)
*   [Features](#-features)
*   [Prerequisites](#-prerequisites)
*   [Installation (CLI)](#-installation-cli)
    *   [Manual Installation](#manual-installation-if-not-using-bashzsh-or-prefer-manual-setup-)
*   [Configuration](#-configuration)
    *   [CLI Configuration (`config.toml`)](#cli-configuration-configtoml)
    *   [MCP Server Configuration (`mcp_servers.json`)](#mcp-server-configuration-mcp_serversjson)
    *   [API Key Precedence](#api-key-precedence)
    *   [System Prompt Precedence](#system-prompt-precedence)
*   [CLI Usage](#-cli-usage)
    *   [Interaction Modes](#-interaction-modes)
    *   [Chat History](#chat-history)
    *   [Memory Features](#-memory-features)
    *   [MCP Integration & Function Calling](#-mcp-integration--function-calling)
*   [Development](#-development)

## 🏗️ Architecture

The Gemini Rust Suite is a modular project composed of several crates within a Cargo workspace:

*   **`gemini-core`**: Provides the foundational components:
    *   Asynchronous `GeminiClient` for interacting with the Google Gemini API (`generateContent` endpoint).
    *   Type-safe Rust structs mirroring the Gemini API request/response formats (content parts, function calls, generation config).
    *   Core configuration loading (`GeminiConfig` from TOML) and error handling (`GeminiError`).
    *   Shared JSON-RPC types (`Request`, `Response`, `ServerCapabilities`, `Tool`) used by MCP.

*   **`gemini-ipc`**: Centralizes Inter-Process Communication definitions:
    *   Defines standardized Rust structs/enums for messages passed between different daemons/clients (e.g., `gemini-cli`, `mcp-hostd`, `HAPPE`, `IDA`).
    *   Ensures consistent communication protocols (relies on `serde` for serialization).

*   **`gemini-mcp`**: Implements the **host** side of the Model Context Protocol (MCP):
    *   `McpHost` manages discovering, launching (via stdio, SSE, WebSocket), and communicating with MCP **servers** (external tools/services).
    *   Handles JSON-RPC communication for tool execution (`mcp/tool/execute`) and resource retrieval.
    *   Translates between Gemini function calling and MCP tool execution.
    *   Includes the `mcp-hostd` binary, a standalone MCP host daemon (uses `gemini-ipc` for client communication).
    *   Provides the source for built-in MCP servers (`filesystem`, `command`, `memory_store`).

*   **`gemini-memory`**: Implements a persistent, semantic memory store:
    *   Uses LanceDB as a vector database.
    *   Stores memories (key-value pairs with metadata) and their vector embeddings.
    *   Performs semantic search to find relevant memories based on query meaning.
    *   Relies on an `McpHostInterface` (typically provided by `gemini-mcp`) to call an external `embedding/embed` tool for generating vectors.
    *   Provides `enhance_prompt` functionality to add relevant memory context to LLM prompts.

*   **`gemini-cli`**: The primary user-facing application:
    *   A command-line interface (`gemini-cli-bin`) built using the other crates.
    *   Supports single-shot prompts, interactive chat, and task loops.
    *   Integrates MCP for tool usage and Memory for context awareness and history.
    *   Manages user configuration, chat history, and session state.
    *   Can connect to the `mcp-hostd` daemon (via `gemini-ipc`) or run an embedded `McpHost`.
    *   Can *also* run the built-in MCP servers directly via flags (`--filesystem-mcp`, etc.).

*   **(New) `HAPPE`**: Host Application Environment daemon:
    *   Intended as the primary execution environment, replacing direct CLI usage for more complex scenarios.
    *   Manages interactions between users/clients, the main LLM, `IDA`, and MCP servers.
    *   Uses `gemini-ipc` to communicate with `IDA`.
    *   Handles LLM calls and LLM-initiated MCP tool execution.

*   **(New) `IDA`**: Internal Dialogue App daemon:
    *   Manages persistent memory and other background cognitive tasks.
    *   Communicates with `HAPPE` via `gemini-ipc`.
    *   Interacts with the Memory MCP Server (via `gemini-mcp` client logic) for retrieval and storage.

## 🚀 Features

This suite provides a comprehensive set of features through its components:

*   **Core API Access**: Robust, async communication with the Gemini API.
*   **Tool Usage (MCP)**: Extend Gemini's capabilities by connecting it to external tools and services via the Model Context Protocol.
*   **Persistent Memory**: Equip Gemini with long-term memory using a semantic vector database (LanceDB), enabling context retrieval across sessions.
*   **Automatic Prompt Enhancement**: Automatically inject relevant memories into prompts.
*   **Auto-Memory Storage**: Automatically capture key information from conversations into the memory store.
*   **Interactive CLI**: User-friendly command-line interface (`gemini`) with multiple interaction modes.
*   **Chat History**: Maintain conversation history across CLI commands (requires shell wrapper function).
*   **Configuration**: Manage API keys, system prompts, models, and feature flags via config files and environment variables.
*   **Formatted Output**: CLI renders markdown and syntax highlighting.
*   **Built-in Tools**: Includes ready-to-use MCP servers for filesystem operations, command execution, and memory storage/embedding.

## ✅ Prerequisites

*   **Rust Toolchain:** Install from [https://rustup.rs/](https://rustup.rs/) 🛠️
*   **Gemini API Key:** Obtain from [Google AI Studio](https://aistudio.google.com/app/apikey) 🔑
*   **Supported Shell (for CLI wrapper):** Bash or Zsh recommended for the seamless `gemini` command experience 🐚
*   **(Optional) External MCP Servers:** If you want to connect to tools beyond the built-in ones.

## 📦 Installation (CLI)

The primary way to use the suite is through the `gemini-cli` application. The easiest installation method uses the provided script:

```bash
# Clone the repository (if you haven't already)
# git clone https://github.com/your-username/gemini-rust-suite
# cd gemini-rust-suite

# Run the installation script from the project root
./install.sh
```

The script will:
1. Check if Rust is installed.
2. Check if the installation directory (`~/.local/bin`) exists and create it if necessary.
3. Build the release binaries (`gemini-cli-bin` and `mcp-hostd`).
4. Install the binaries to `~/.local/bin/`.
5. Attempt to install MCP server wrapper scripts (e.g., for built-in servers) using `install_mcp_servers.sh` if found.
6. **Add a wrapper function named `gemini`** to your `~/.bashrc` or `~/.zshrc` (if detected). This wrapper is crucial for managing session history across separate commands.
7. **(Zsh only)** Add a helper function named `mcpd` to manage the `mcp-hostd` daemon (`mcpd start`, `mcpd stop`, `mcpd status`, `mcpd logs`).
8. Prompt you to reload your shell configuration (e.g., `source ~/.zshrc`).

**Important:** You *must* reload your shell configuration after installation for the `gemini` command (and `mcpd` for Zsh users) to become available. 🔄

### Manual Installation (If not using Bash/Zsh or prefer manual setup) 🔧

1. Build the required binaries:
   ```bash
   cargo build --release --bin gemini-cli-bin --bin mcp-hostd
   ```
2. Copy the binaries to a location in your PATH:
   ```bash
   cp target/release/gemini-cli-bin ~/.local/bin/
   cp target/release/mcp-hostd ~/.local/bin/
   ```
3. Ensure the chosen location (e.g., `~/.local/bin`) is in your PATH.
4. **Crucially**, manually add the `gemini` wrapper function (found within the `install.sh` script or the section below) to your shell config file (`.bashrc`, `.zshrc`, etc.) to enable session history across commands. Without the wrapper, history will only persist within a single interactive (`-i`) session.
   ```bash
   # Gemini CLI Wrapper Function Start
   # This function wraps the gemini-cli-bin to manage session environment variables
   gemini() {
       # Path to the actual binary
       local gemini_bin="$HOME/.local/bin/gemini-cli-bin"
       
       # Check if binary exists
       if [ ! -x "$gemini_bin" ]; then
           echo "Error: gemini-cli-bin not found or not executable at $gemini_bin" >&2
           return 1
       fi

       # Run the actual binary, capturing output
       local output
       # Use eval to handle potential quotes in arguments correctly
       output=$(eval "$gemini_bin \"\$@\"") 
       local exit_code=$?

       # Extract export commands from the output if the command was successful
       if [ $exit_code -eq 0 ]; then
           # Filter lines starting with # export
           local exports
           exports=$(echo "$output" | grep '^# export')

           # Execute the export commands if any were found
           if [ -n "$exports" ]; then
               # Remove the comment and execute
               eval "$(echo "$exports" | sed 's/^# export //')"
           fi
           
           # Print the output, excluding the export lines
           echo "$output" | grep -v '^# export'
       else
           # If the command failed, just print the output (likely error messages)
           echo "$output"
       fi

       return $exit_code
   }
   # Gemini CLI Wrapper Function End
   ```
5. Reload your shell configuration.

## ⚙️ Configuration

The suite uses a couple of configuration files, primarily managed by the CLI:

### CLI Configuration (`config.toml`)

*   **Location:** `~/.config/gemini-suite/config.toml`
*   **Purpose:** Unified configuration for CLI, Daemons (HAPPE, IDA, mcp-hostd).
*   **Managed by:** `gemini config` or `gemini daemon-manager config` subcommands.
*   **Details:** See `cli/README.md` and `daemon-manager/README.md`.

### MCP Server Configuration (`mcp_servers.json`)

*   **Location:** `~/.config/gemini-suite/mcp_servers.json`
*   **Purpose:** Defines how the MCP Host connects to external tool servers.
*   **Managed by:** `gemini mcp` or `gemini daemon-manager mcp` subcommands.

### API Key Precedence 🔑

1.  Value in `~/.config/gemini-suite/config.toml`.
2.  `GEMINI_API_KEY` environment variable.
3.  Value in a `.env` file in the current working directory (or project root during development).

### System Prompt Precedence 🗣️

1.  Value in `~/.config/gemini-suite/config.toml`.
2.  Default prompt embedded in the CLI.

## 💡 CLI Usage

Use the `gemini` wrapper command (after installation and shell reload).

```bash
# Basic prompt
gemini "Explain Rust's ownership model."

# Interactive chat mode (maintains history within the session)
gemini -i
# Type '/exit' or press Ctrl+C to quit

# Using the wrapper for history across commands:
# First command (creates a session ID):
gemini "What is the capital of France?"
# Subsequent command uses the history (if shell reloaded/wrapper sourced):
gemini "What language do they speak there?"

# Start a new conversation thread (gets a new session ID)
gemini --new-chat "Tell me about Tokio."

# Ask Gemini for command help (requires 'command' MCP server)
gemini -c "list files sorted by modification time"

# Ask Gemini to use a tool (requires relevant MCP server)
gemini "Read the contents of my main.rs file."

# Task loop mode
gemini -t "Refactor the Rust code in src/utils.rs to improve error handling."

# Show configuration
gemini --show-config

# Get help on all flags
gemini --help

# Manage the MCP Host Daemon (Zsh only, via install.sh helper function)
mcpd start
mcpd status
mcpd stop
mcpd logs
```

### Interaction Modes

*   **Single Shot (Default):** Send one prompt, get one response.
*   **Interactive Chat (`-i`):** REPL-style conversation within a single CLI execution. History persists within this mode.
*   **Task Loop (`-t`):** Assign a complex task for Gemini to work on autonomously, potentially using tools and asking for clarification only when needed.

### Chat History

*   History is saved to files in `~/.local/share/gemini-suite/history/`.
*   Requires the wrapper function (installed by the installer) to work correctly across shell sessions.

### Memory Features

Powered by `gemini-memory` and LanceDB (`~/.local/share/gemini-suite/memory.db`).

*   **Memory Broker (`--enable-memory-broker` / `--disable-memory-broker`):** Automatically retrieves relevant past memories and adds them as context to your prompts.
*   **Auto Memory (`--enable-auto-memory` / `--disable-auto-memory`):** Automatically extracts and saves key information from conversations.
*   Requires an embedding service, typically provided by the built-in `memory-store-mcp` server or an external equivalent configured via `mcp_servers.json`.

### MCP Integration & Function Calling

Powered by `gemini-mcp`.

*   The CLI connects to MCP servers defined in `mcp_servers.json` (either via the `mcp-hostd` daemon or an internal host).
*   Discovered tools are presented to Gemini.
*   When Gemini decides to use a tool:
    1.  The CLI displays the proposed tool call (e.g., `filesystem.readFile(path="./README.md")`).
    2.  It **prompts you for confirmation (y/n)** before executing.
    3.  If confirmed, the tool is executed via the MCP host/server.
    4.  The result is sent back to Gemini.
*   **Security:** Always review tool calls before confirming, especially for `command` execution or filesystem modifications.
*   **Built-in Servers:** The CLI binary itself can run the included servers:
    *   `gemini --filesystem-mcp`
    *   `gemini --command-mcp`
    *   `gemini --memory-store-mcp` (Provides embedding and storage for the Memory features)
    (These flags run the server exclusively; they don't accept prompts.)
*   **Daemon Management:** The `mcp-hostd` binary is the standalone daemon. You can manage it directly (e.g., `mcp-hostd &`) or use the `mcpd` helper function added by `install.sh` for Zsh users (`mcpd start`, `mcpd stop`, `mcpd status`, `mcpd logs`).

## 💻 Development

This project is a Cargo workspace.

```bash
# Clone the repository
# git clone https://github.com/your-username/gemini-rust-suite
# cd gemini-rust-suite

# Build all crates
cargo build

# Build the CLI specifically (release mode)
cargo build --release --package gemini-cli

# Run the CLI directly from the workspace root
# (Ensure API key is set via .env in workspace root or exported env var)
cargo run --package gemini-cli -- "Your prompt"

cargo run --package gemini-cli -- -i # Interactive mode

cargo run --package gemini-cli -- -c "find text in files" # Command help

# Run tests for all crates
cargo test

# Run tests for a specific crate
cargo test --package gemini-core

# Run the MCP Host Daemon directly
cargo run --package gemini-mcp --bin mcp-hostd
``` 