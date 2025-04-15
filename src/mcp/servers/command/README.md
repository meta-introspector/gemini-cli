# Command MCP Server

This is an MCP (Model Context Protocol) server that provides command execution capabilities to MCP clients. It allows models to execute system commands and shell scripts in a controlled and standardized way.

## Capabilities

### Tools

1. **execute_command**
   - Executes a system command with specified arguments
   - Parameters:
     - `command` (string, required): The command to execute
     - `arguments` (array of strings, optional): Arguments to pass to the command
     - `working_directory` (string, optional): Working directory for the command
     - `environment` (object, optional): Environment variables for the command
     - `timeout_ms` (integer, optional): Maximum execution time in milliseconds (0 for no timeout)

2. **execute_shell**
   - Executes a shell command using the default system shell (sh on Unix-like systems, cmd on Windows)
   - Parameters:
     - `command` (string, required): The shell command to execute
     - `working_directory` (string, optional): Working directory for the command
     - `environment` (object, optional): Environment variables for the command
     - `timeout_ms` (integer, optional): Maximum execution time in milliseconds (0 for no timeout)

### Resources

1. **os_info**
   - Gets information about the operating system
   - Returns OS type, release, version, architecture, and family

2. **environment_variables**
   - Gets the current environment variables
   - Returns a map of all environment variables

## Security Considerations

The command MCP server runs commands with the same permissions as the user running the MCP client. This means it can potentially perform any action that the user can perform. Consider the following security practices:

1. **User Consent**: Always require explicit user consent before executing commands.
2. **Command Validation**: Validate commands before execution to prevent potential security issues.
3. **Restricted Environments**: Consider running the server in a restricted environment when used in production.

## Usage

### Building and Installing

Run the build script to compile and install the server:

```bash
./build.sh
```

This will install the binary and wrapper script to `~/.local/bin/`.

### Integration with MCP Clients

To use this server with an MCP client, configure the client to connect to the server using the stdio transport. For example, with gemini-cli, you would add this server to your configuration.

### Example: Using with gemini-cli

1. Add the server to your `~/.config/gemini-cli/server_config.json`:

```json
{
  "servers": [
    {
      "name": "command",
      "transport": "stdio",
      "command": "command-mcp"
    }
  ]
}
```

2. Then you can use the command execution tools in your interactions with the model.

## Response Format

The command execution tools return a structured response with the following fields:

- `exit_code`: The exit code of the command (0 usually means success)
- `stdout`: The standard output of the command
- `stderr`: The standard error output of the command
- `success`: A boolean indicating whether the command was successful (based on exit code) 