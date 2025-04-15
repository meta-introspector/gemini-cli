# Filesystem MCP Server

This is a basic filesystem operations server implementing the Model Context Protocol (MCP) to be used with gemini-cli.

## Features

The server provides several tools and resources for working with the local filesystem:

### Tools

1. **list_directory** - Lists contents of a directory
   - Parameters: 
     - `path` (string, required): Path to the directory to list
     - `recursive` (boolean, optional): Whether to list subdirectories recursively (default: false)

2. **read_file** - Reads content of a file
   - Parameters:
     - `path` (string, required): Path to the file to read
     - `encoding` (string, optional): File encoding (default: utf-8)

3. **write_file** - Writes content to a file
   - Parameters:
     - `path` (string, required): Path to the file to write
     - `content` (string, required): Content to write to the file
     - `mode` (string, optional): Write mode - 'create', 'append', or 'overwrite' (default: overwrite)

4. **delete** - Deletes a file or directory
   - Parameters:
     - `path` (string, required): Path to the file or directory to delete
     - `recursive` (boolean, optional): Whether to recursively delete directories (default: false)

5. **create_directory** - Creates a new directory
   - Parameters:
     - `path` (string, required): Path to the directory to create
     - `recursive` (boolean, optional): Whether to create parent directories if they don't exist (default: false)

6. **file_info** - Gets information about a file or directory
   - Parameters:
     - `path` (string, required): Path to the file or directory

### Resources

1. **current_directory** - Gets the current working directory
2. **home_directory** - Gets the user's home directory

## Setup

### Building

```bash
cd src/mcp/servers/filesystem
cargo build --release
```

### Configuration

Add the following to your gemini-cli's MCP configuration file:

```json
{
  "name": "filesystem",
  "enabled": true,
  "transport": "stdio",
  "command": ["path/to/filesystem-mcp"],
  "args": []
}
```

Replace `path/to/filesystem-mcp` with the actual path to the compiled binary.

## Security Considerations

This server provides direct access to the local filesystem with the permissions of the running user. Be cautious when using this in production environments, as it can:

1. Read potentially sensitive files
2. Modify or delete existing files and directories
3. Create new files and directories

Always verify tool operations before approving them, especially for write, delete, or create operations. 