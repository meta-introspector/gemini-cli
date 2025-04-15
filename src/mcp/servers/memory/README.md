# Memory MCP Server

This MCP server provides persistent memory capabilities to the Gemini CLI. It allows storing and retrieving key-value based memories, with intelligent storage and retrieval using Gemini-2.0-flash as a memory broker.

## Features

- **Persistent Storage**: Store key-value pairs with associated tags in a persistent JSON file
- **Intelligent Retrieval**: Use Gemini-2.0-flash to determine which memories are relevant to a user query
- **Memory Management**: Add, retrieve, delete, and list memories with simple commands
- **Automatic Memory Extraction**: Extract important information from conversations automatically

## Tools

The memory MCP server provides the following tools:

- `store_memory`: Store a new memory in the persistent memory store
- `retrieve_memory`: Retrieve memories by key
- `retrieve_by_tag`: Retrieve memories by tag
- `list_all_memories`: List all stored memories
- `delete_memory`: Delete a memory by key
- `get_relevant_memories`: Get memories relevant to a query using Gemini-2.0-flash

## Resources

The memory MCP server provides the following resources:

- `memory_stats`: Get statistics about the memory store
- `memory_schema`: Get the schema of memory objects

## Usage

The memory server is used internally by the Gemini CLI to enhance queries with relevant context and store important information from conversations.

### Example: Store a memory

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "mcp/tool/execute",
  "params": {
    "tool_name": "store_memory", 
    "arguments": {
      "key": "project_deadline",
      "value": "The project needs to be completed by June 15th, 2024",
      "tags": ["project", "deadline", "important"]
    }
  }
}
```

### Example: Get relevant memories for a query

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "mcp/tool/execute",
  "params": {
    "tool_name": "get_relevant_memories",
    "arguments": {
      "query": "When is my project due?",
      "api_key": "YOUR_GEMINI_API_KEY"
    }
  }
}
```

## Configuration

To enable the memory MCP server, add the following to your MCP server configuration:

```json
{
  "name": "memory",
  "enabled": true,
  "transport": "stdio",
  "command": ["~/.local/bin/mcp-servers/memory-mcp"],
  "args": [],
  "auto_execute": []
}
```

## Storage Location

Memory is stored in a JSON file at `~/.config/gemini-cli/memory_store.json`.

## Security Considerations

-   The memory store (`memory_store.json`) might contain sensitive information extracted from conversations. Ensure the file has appropriate permissions.
-   The `get_relevant_memories` and `store_memory` tools make calls to the Gemini API (using the provided `api_key`). Ensure your API key is kept secure. 