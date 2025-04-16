# Gemini Rust CLI 🦀

A command-line interface (CLI) tool written in Rust to interact with Google Gemini models from the Linux terminal.

## ✨ Table of Contents

*   [Features](#-features)
*   [Prerequisites](#-prerequisites)
*   [Installation](#-installation)
    *   [Manual Installation](#manual-installation-if-not-using-bashzsh-or-prefer-manual-setup-)
*   [Configuration](#-configuration)
    *   [Command History Context](#command-history-context)
    *   [Chat History](#chat-history)
    *   [Automatic History Summarization](#automatic-history-summarization)
    *   [API Key Precedence](#api-key-precedence)
    *   [System Prompt Precedence](#system-prompt-precedence)
*   [Memory Features](#-memory-features)
    *   [Memory Broker](#memory-broker)
    *   [Auto Memory](#auto-memory)
    *   [Deduplication Tool](#deduplication-tool)
    *   [How It Works](#how-it-works)
*   [MCP Integration](#-mcp-integration)
    *   [Included MCP Servers](#included-mcp-servers)
        *   [Filesystem MCP Server](#filesystem-mcp-server)
        *   [Command MCP Server](#command-mcp-server)
    *   [External MCP Servers](#external-mcp-servers)
    *   [MCP Server Installation](#mcp-server-installation)
    *   [MCP Configuration](#mcp-configuration)
    *   [Security Considerations](#-security-considerations)
    *   [Function Calling](#-function-calling)
    *   [Resource Access](#-resource-access)
*   [Interaction Modes](#-interaction-modes)
    *   [Single Shot Mode](#single-shot-mode)
    *   [Interactive Chat Mode](#interactive-chat-mode)
    *   [Task Loop Mode](#task-loop-mode)
*   [Usage](#-usage)
*   [Development](#-development)

## 🚀 Features

*   💬 Send prompts to Gemini models.
*   📜 View the last 5 commands from your shell history for better context.
*   🧠 **In-memory conversation history** for continuous conversations in the same terminal session.
*   ✍️ **Automatic conversation summarization** when token count gets too large.
*   🗨️ **Interactive chat mode** (`-i`) for continuous conversation with the model.
*   🔁 **Task Loop mode** (`-t`) for persistent task execution with AI-controlled flow.
*   ⚙️ Persistent configuration for API Key and System Prompt.
*   🔑 Configure API key via config file, environment variable, or command-line flag (for setting).
*   💻 Special flag (`-c`) to request Linux command help. **The CLI will propose a command and ask for your confirmation before executing it.**
*   🔗 **MCP Integration** for function calling and tool execution.
*   📦 **Resource access** through MCP servers.

## ✅ Prerequisites

*   **Rust Toolchain:** Install from [https://rustup.rs/](https://rustup.rs/) 🛠️
*   **Gemini API Key:** Obtain from [Google AI Studio](https://aistudio.google.com/app/apikey) 🔑
*   **Supported Shell:** Bash or Zsh (for automatic wrapper function installation) 🐚
*   **MCP Servers:** (Optional) For function calling and resource access 🔌

## 📦 Installation

The easiest way to install is to use the provided installation script:

```bash
# Clone the repository
git clone https://github.com/frostdev-ops/gemini-cli
cd gemini-cli

# Run the installation script from project root or parent directory
./install.sh
```

The script will:
1. Check if Rust is installed.
2. Build the release binary (`gemini-cli-bin`).
3. Install the binary to `~/.local/bin/gemini-cli-bin`.
4. **Add a wrapper function named `gemini`** to your `~/.bashrc` or `~/.zshrc`.
5. Prompt you to reload your shell configuration (e.g., `source ~/.zshrc`).

**Important:** You *must* reload your shell configuration after installation for the `gemini` command (the wrapper function) to become available. 🔄

### Manual Installation (If not using Bash/Zsh or prefer manual setup) 🔧

1. Build the binary: `cargo build --release`
2. Copy the binary: `cp target/release/gemini-cli ~/.local/bin/gemini-cli-bin`
3. Ensure `~/.local/bin` is in your PATH.
4. Manually add the following wrapper function to your shell config file:
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

The CLI uses a configuration file typically located at `~/.config/gemini-cli/config.toml`.

You can manage the configuration using these flags:

*   **Set API Key:** Saves the key persistently.
    ```bash
    gemini --set-api-key YOUR_API_KEY_HERE
    ```
*   **Set System Prompt:** Saves the default instructions for the AI.
    ```bash
    gemini --set-system-prompt "You are a Rust programming expert."
    ```
*   **Show Configuration:** Displays the current settings.
    ```bash
    gemini --show-config
    ```

**Command History Context:** 📜
By default, Gemini CLI will access your last 5 terminal commands to provide context to the AI. This helps Gemini provide more relevant responses, especially for command-related queries.

**Chat History:** 💬
Gemini CLI maintains conversation history within a terminal session using files in your config directory. This allows for back-and-forth conversations.

The **`gemini` command uses the terminal session information** to identify conversations, but since each command runs in a separate process, you need to export a session ID variable to maintain history across multiple commands:

* To maintain history across separate commands, run the export command shown after your first interaction:
  ```bash
  export GEMINI_SESSION_ID="your_generated_session_id"
  ```
  This ID will be shown when you run Gemini with DEBUG mode or when a default session ID is used.

* To start a new conversation: `gemini --new-chat "Hello"`
* To disable conversation history: `gemini --disable-history`
* To enable conversation history: `gemini --enable-history`

**Automatic History Summarization:** ✍️
When a conversation gets too long (exceeding 700,000 estimated tokens), Gemini CLI will automatically summarize the conversation to reduce token usage while preserving the key context and information. This ensures that:

1. Long conversations remain manageable
2. You stay within API token limits
3. Context from earlier in the conversation is preserved in a condensed form

For long conversations across multiple commands, you may need to run the export commands shown in the output to maintain history across commands.

To see the current conversation history, token estimates, and system prompt (for debugging):
```bash
GEMINI_DEBUG=1 gemini "your prompt"
```

**API Key Precedence:** 🔑
1.  Value set in the configuration file (`~/.config/gemini-cli/config.toml`).
2.  `GEMINI_API_KEY` environment variable.

**System Prompt Precedence:** 🗣️
1.  Value set in the configuration file.
2.  Default: "You are a helpful assistant."

**Note:** The `dotenv` crate is still used, so a `.env` file in the **current working directory** (where you run `gemini`) or the **project root** (during development) can set the `GEMINI_API_KEY` environment variable if it's not set globally or in the config file.

## 🧠 Memory Features

The Gemini CLI now supports advanced memory features that allow Gemini to remember and recall information across conversations:

### Memory Broker

The memory broker enhances your queries by retrieving relevant information from past interactions:

- **Automatic Relevance Filtering**: Only memories relevant to your current query are included 🎯
- **Seamless Integration**: Relevant memories are provided as context to the model without changing your prompt ✨
- **Customizable Model**: Control which model is used for relevance filtering 🔧
- **Memory Deduplication**: Automatically detects and removes duplicate memories to keep the memory store clean 🧹
- **Improved Context Integration**: Better formatting of memory context for more natural responses 👍

To control the memory broker:
- Enable memory broker: `gemini --enable-memory-broker`
- Disable memory broker: `gemini --disable-memory-broker`
- Check status: `gemini --show-config`

### Auto Memory

The auto memory feature automatically extracts and stores important information from conversations:

- **Key Information Extraction**: Identifies facts, preferences, and details worth remembering 📝
- **Contextual Storage**: Automatically categorizes information with relevant tags 🏷️
- **Smart Filtering**: Only stores truly important information, not casual conversation 🧠
- **Duplicate Prevention**: Avoids creating duplicate entries for the same information 🚫
- **Tag Merging**: Intelligently merges tags when updating existing memories 🔄

To control the auto memory feature:
- Enable auto memory: `gemini --enable-auto-memory`
- Disable auto memory: `gemini --disable-auto-memory`
- Check status: `gemini --show-config`

### Deduplication Tool

The memory system now includes a dedicated deduplication tool that:

- Removes redundant memories while keeping the most recent version 🧹
- Maintains a clean and efficient memory store ✨
- Runs automatically on startup and periodically during usage ⚙️
- Can be triggered manually when needed 👆

The memory system intelligently manages duplicates by:
1. Checking for exact key/value matches before storing
2. Updating existing entries instead of creating duplicates
3. Merging tags from multiple entries to preserve all context
4. Periodically cleaning up the memory store

### How It Works

1. **When you ask a question** ❓:
   - The memory broker retrieves all memories from the store 📚
   - Periodically deduplicates the memory store to prevent clutter 🧹
   - Filters memories for relevance to your query using a specialized model 🎯
   - Enhances your query with properly formatted relevant information ✨

2. **When you get a response** 💬:
   - The auto memory system extracts key information 📝
   - Checks if similar information already exists 🤔
   - Updates existing entries or creates new ones as appropriate 💾
   - Tags the information with relevant categories for future retrieval 🏷️

3. **On future queries** ➡️:
   - Relevant memories are automatically included to provide continuity 🔗
   - The system becomes more useful over time as it builds a personal knowledge base 📈
   - Duplicate information is consolidated to maintain a clean memory store ✅

This creates a system that gets more useful over time while remaining efficient and focused on the most relevant information for your needs.

## 🔗 MCP Integration

The Gemini CLI now supports the Model Context Protocol (MCP) for function calling and resource access. This allows Gemini to:

1. **Execute tools** through MCP servers 🛠️
2. **Access resources** provided by MCP servers 📦
3. **Combine capabilities** from multiple MCP servers 🤝

### Included MCP Servers

#### Filesystem MCP Server 📁

The Gemini CLI includes a built-in filesystem MCP server that provides file and directory operations:

- **List directory contents** with optional recursive traversal 📂
- **Read and write files** with various modes (create, append, overwrite) 📝
- **Create and delete** files and directories ➕➖
- **Get file information** including size, type, and modification times ℹ️
- **Access environment directories** like current working directory and home directory 🏠

This server enables Gemini to help with file management tasks directly from the CLI.

#### Command MCP Server 💻

The Gemini CLI also includes a built-in command execution MCP server that allows executing system commands:

- **Execute commands** with arguments, working directory, and environment variables ▶️
- **Execute shell commands** using the system's default shell 🐚
- **Get OS information** including OS type, version, architecture 🖥️
- **Access environment variables** available to the current process 🌍

This server enables Gemini to help with running commands and scripts directly from the CLI.

### External MCP Servers

Gemini CLI can connect to and use multiple MCP servers simultaneously:

1. **Built-in servers**: The filesystem and command servers are included with gemini-cli 📦
2. **Local servers**: You can run custom MCP servers locally and connect via stdio 🔌
3. **Remote servers**: You can connect to remote MCP servers using HTTP+SSE transport 🌐

When a server is configured, gemini-cli:
1. Connects to the server during startup 🔌
2. Discovers its tools and resources through the MCP protocol 🔍
3. Makes those tools and resources available to the Gemini model 🤖
4. Handles executing tools and fetching resources when requested by the model 🚀

### MCP Server Installation

The built-in MCP servers are automatically installed when you run the installation script:

```bash
./install.sh
```

This script:
1. Builds and installs the gemini-cli binary 🏗️
2. Creates wrapper scripts for the filesystem and command MCP servers 📜
3. Sets up a default MCP server configuration ⚙️

To install custom MCP servers:
1. Build or acquire the MCP server executable 📦
2. Add it to your PATH or specify its full path in the configuration 🗺️
3. Update your MCP server configuration (see below) 📝

### MCP Configuration

MCP servers can be configured in the `~/.config/gemini-cli/mcp_servers.json` file:

```json
[
  {
    "name": "filesystem",
    "enabled": true,
    "transport": "stdio",
    "command": ["~/.local/bin/mcp-servers/filesystem-mcp"],
    "args": []
  },
  {
    "name": "command",
    "enabled": true,
    "transport": "stdio",
    "command": ["~/.local/bin/mcp-servers/command-mcp"],
    "args": []
  },
  {
    "name": "example-server",
    "enabled": true,
    "transport": "stdio",
    "command": ["path/to/server/binary"],
    "args": ["--config", "path/to/config.json"],
    "env": {
      "SERVER_ENV_VAR": "value"
    }
  },
  {
    "name": "remote-server",
    "enabled": true,
    "transport": "sse",
    "url": "http://localhost:8080/sse",
    "headers": {
      "Authorization": "Bearer token"
    }
  }
]
```

Configuration fields:
- **name**: Unique identifier for the server
- **enabled**: Whether the server should be loaded (true/false)
- **transport**: Connection method ("stdio" or "sse")
- **command**: Command and arguments to start the server (for stdio transport)
- **args**: Additional arguments for the command (for stdio transport)
- **env**: Environment variables to set for the server process (for stdio transport)
- **url**: Server endpoint URL (for sse transport)
- **headers**: HTTP headers to send (for sse transport)

### 🔒 Security Considerations

MCP servers, especially the command execution server, can have significant security implications:

1. **User Consent**: gemini-cli always asks for user confirmation before executing any tool 👍
2. **Permissions**: MCP servers run with the same permissions as the gemini-cli process 🔑
3. **Command Validation**: Always review commands before allowing them to execute 👀
4. **Tool Access Control**: Disable MCP servers you don't need or trust 🚫

For the command MCP server specifically:
- Review all commands before execution
- Be careful with commands that modify files or system settings
- Consider the working directory and environment context
- Avoid executing commands that could expose sensitive information

### 🛠️ Function Calling

When Gemini detects that a tool should be used, it will:

1. Propose the tool execution with arguments
2. Ask for your confirmation before executing
3. Execute the tool if confirmed
4. Return the results to Gemini for further processing

Example using the filesystem MCP server:
```
User: "Find all Python files in the current directory"
Gemini: I'll help you find Python files in the current directory. I'll use the file system tool to do this.

I'll execute: list_directory(path=".", recursive=true)
Would you like me to execute this command? (y/n): y

Found 5 Python files:
- ./src/main.py
- ./src/utils.py
- ./tests/test_main.py
- ./tests/test_utils.py
- ./setup.py
```

Example using the command MCP server:
```
User: "Show me the current disk usage"
Gemini: I'll help you check the disk usage. I'll use the command execution tool for this.

I'll execute: execute_shell(command="df -h")
Would you like me to execute this command? (y/n): y

Filesystem      Size  Used Avail Use% Mounted on
/dev/sda1       234G   67G  156G  31% /
/dev/sdb1       932G  412G  474G  47% /data
```

### 📦 Resource Access

Gemini can also access resources provided by MCP servers, such as:

- File system information 📁
- System metrics and OS information 📊
- Environment variables 🌍
- Network status 🌐
- And more, depending on the MCP server implementation 🧩

## 🗣️ Interaction Modes

Gemini CLI supports multiple ways to interact with the model:

### Single Shot Mode 🎯

This is the default mode, where you provide a single prompt and get a response:

```bash
gemini "How do I list all files in a directory, including hidden files?"
```

### Interactive Chat Mode 💬

For continuous back-and-forth conversation with Gemini, use the `-i` flag:

```bash
gemini -i
```

This starts an interactive session where you can chat with Gemini continuously without restarting the CLI. Type `exit` or `quit` to end the session, or press `Ctrl+C` to terminate immediately.

### Task Loop Mode 🔁

The Task Loop mode allows Gemini to work on a complex task autonomously, asking for input only when necessary:

```bash
gemini -t "Create a Python script that downloads weather data for London and displays it in a chart"
```

In Task Loop mode:

- Gemini works autonomously on your task, showing progress updates 🏃
- It uses available tools (like accessing files or running commands) as needed 🛠️
- It only asks for input when it needs specific information from you 🤔
- It signals completion when the task is finished or when it gets stuck ✅ / ❌

**Flow Control Signals:**
- When Gemini needs your input, it will pause and wait for you to respond ⏸️
- When the task is complete, you'll see a "✅ Task Complete" message with a summary 🎉
- If Gemini gets stuck, you'll see a "❌ Task Stuck" message with the reason 🤷

You can exit the Task Loop at any time by typing `exit` or `quit` when prompted for input, or by pressing `Ctrl+C`.

This mode is especially powerful for complex tasks that might require multiple steps, tool usage, or occasional user input.

## 💡 Usage

```bash
# Configure first (if needed)
gemini --set-api-key YOUR_API_KEY_HERE
gemini --set-system-prompt "Be concise."

# Basic prompt (no flag needed)
gemini "Explain quantum physics simply"

# Continue the conversation in the same session
gemini "What's a practical application of that?"

# Start a new conversation
gemini --new-chat "How do I set up SSH keys?"

# Using command help flag
# This will ask Gemini for a command, display it, and prompt for confirmation before running.
gemini -c "list files sorted by size"

# Interactive chat mode
gemini -i

# Task loop mode
gemini -t "Write a bash script to back up my Documents folder daily"

# Manage history
gemini --disable-history
gemini --enable-history

# Show current config
gemini --show-config

# Get help
gemini --help
```

## 💻 Development

Run directly using `cargo run`:

```bash
# Make sure you have a .env file in gemini-cli/ or export GEMINI_API_KEY
# Run from the workspace root (/home/james/Documents/gemini-cli)

cargo run --manifest-path gemini-cli/Cargo.toml -- "Your prompt"

cargo run --manifest-path gemini-cli/Cargo.toml -- -c "find text in files"
``` 