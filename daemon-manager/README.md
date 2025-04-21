# Gemini Suite Daemon Manager

A command-line tool for managing gemini-suite daemons and MCP servers.

## Features

- **Daemon Management**: Start, stop, restart, and check status of the HAPPE, IDA, and MCP host daemons.
- **Daemon Installation**: Install and uninstall daemons as systemd user services.
- **MCP Server Management**: Enable, disable, list, and check status of MCP servers.
- **MCP Server Installation**: Install and uninstall MCP servers.
- **Configuration Management**: Edit, show, and reset configuration files for all components.
- **Unified Startup/Shutdown**: Start and stop all daemons in the correct order with single commands.
- **System Status**: Get a comprehensive view of all components with a single command.
- **Location Independent**: Works from any directory after installation, not just the project directory.

## Installation

The `gemini-manager` tool is installed as part of the main Gemini Suite installation process.

From the root of the gemini-suite workspace:

```bash
# Build the installer
cargo build --release --package install

# Run the installer (installs CLI, Daemons, and Manager by default)
./target/release/gemini-installer install

# Or install only the manager
./target/release/gemini-installer install-manager
```

This will build the binary in release mode and install it to `~/.local/bin/gemini-manager` (by default).

After installation, you can run `gemini-manager` from any directory, not just the project directory.

## Usage

```bash
# Show help
gemini-manager --help

# Show comprehensive status of all components
gemini-manager status

# Start all daemons in the correct order (mcp-hostd -> ida -> happe)
gemini-manager start

# Stop all daemons in reverse order (happe -> ida -> mcp-hostd)
gemini-manager stop

# Daemon management
gemini-manager daemon start happe
gemini-manager daemon stop ida
gemini-manager daemon restart mcp-hostd
gemini-manager daemon status happe
gemini-manager daemon install happe
gemini-manager daemon uninstall happe
gemini-manager daemon list

# MCP server management
gemini-manager mcp list
gemini-manager mcp enable filesystem
gemini-manager mcp disable command
gemini-manager mcp status memory-store
gemini-manager mcp install /path/to/server
gemini-manager mcp uninstall custom-server

# Configuration management
gemini-manager config edit mcp-servers
gemini-manager config show cli
gemini-manager config reset happe
```

## Status Command

The `gemini-manager status` command provides a comprehensive view of your Gemini Suite installation:

- Shows the running status of all daemons (mcp-hostd, ida, happe)
- Lists all MCP servers, both built-in and custom, with their enabled/disabled status
- Includes helpful command shortcuts for common management tasks

Example output:
```
=== Gemini Suite Status ===

Daemons:
  mcp-hostd : Running
  ida       : Stopped
  happe     : Stopped

MCP Servers:
  filesystem   : Disabled (built-in)
  command      : Enabled (built-in)
  memory-store : Enabled (built-in)
  embedding    : Enabled

Management:
  Start all    : gemini-manager start
  Stop all     : gemini-manager stop
  Configure    : gemini-manager config edit <component>
```

## Daemon Installation Details

When installing daemons as systemd services, the tool creates user systemd service files in `~/.config/systemd/user/`. These services are managed by the user's systemd instance, not the system-wide one.

The services are configured to:
- Start automatically at login (if enabled)
- Restart on failure
- Use the appropriate binary path

## MCP Server Configuration

MCP servers are configured in `~/.config/gemini-cli/mcp_servers.json`. This tool provides a convenient interface for managing this configuration.

Built-in servers (filesystem, command, memory-store) are always available and can be enabled/disabled as needed.

## Configuration Files

The following configuration files are managed by this tool:

- `~/.config/gemini-cli/config.toml`: Gemini CLI configuration
- `~/.config/gemini-cli/mcp_servers.json`: MCP server configuration
- `~/.config/gemini-cli/happe.toml`: HAPPE daemon configuration
- `~/.config/gemini-cli/ida.toml`: IDA daemon configuration
- `~/.config/gemini-cli/mcp-hostd.toml`: MCP host daemon configuration

## Startup and Shutdown Order

When using the `gemini-manager start` command, daemons are started in the following order with appropriate pauses between each:

1. `mcp-hostd` - The MCP host daemon that manages tool servers
2. `ida` - The Internal Dialogue App daemon that manages persistent memory 
3. `happe` - The Host Application Environment daemon that orchestrates everything

When using the `gemini-manager stop` command, daemons are stopped in the reverse order:

1. `happe` - Stop the main application first
2. `ida` - Stop the memory service next
3. `mcp-hostd` - Stop the tool server host last 

# MCP Server Configuration Format

The `gemini-manager` now supports Claude-compatible MCP server configuration format. This format is used by Claude Desktop and other MCP clients.

## Claude-Compatible Format

```json
{
  "mcpServers": {
    "context7": {
      "command": "npx",
      "args": [
        "-y",
        "@upstash/context7-mcp@latest"
      ]
    },
    "supabase": {
      "command": "npx",
      "args": [
        "-y",
        "@supabase/mcp-server-supabase@latest",
        "--access-token",
        "your-access-token-here"
      ]
    },
    "mcp-server-docker": {
      "command": "docker",
      "args": [
        "run",
        "-i",
        "--rm",
        "-v",
        "/var/run/docker.sock:/var/run/docker.sock",
        "mcp-server-docker:latest"
      ]
    }
  }
}
```

Key features of this format:
- Uses a top-level `mcpServers` object
- Server names are keys in the object, not fields in each server entry
- Command is a string, not an array
- Arguments are separated into an `args` array

## Migrating to Claude-Compatible Format

If you have existing MCP server configurations in the legacy format, you can migrate them to the Claude-compatible format using:

```bash
gemini-manager mcp migrate
```

This will convert your configuration while preserving all your settings. 