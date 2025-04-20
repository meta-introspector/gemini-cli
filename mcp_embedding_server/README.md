# E5 Embedding MCP Server

This is a Model Context Protocol (MCP) server that provides text embedding functionality for the Gemini CLI using the E5 embedding models from Microsoft.

## Features

- Provides text embeddings using the E5 multilingual models (small, base, or large variants)
- Uses stdout/stdin JSON-RPC communication for lightweight integration with the Gemini MCP system
- Automatically prefixes inputs with "query:" or "passage:" according to E5 requirements
- Lazy-loads the model on first request to minimize startup time
- Allows runtime model switching if needed

## Setup

1. Install the required dependencies:

```bash
pip install -r requirements.txt
```

2. Make the server script executable:

```bash
chmod +x server.py
```

## Usage

The server is designed to be launched by the Gemini MCP host system. Add it to your MCP configuration:

```toml
[[mcp.servers]]
name = "embedding"
enabled = true
transport = "stdio"
command = "/path/to/mcp_embedding_server/server.py"
args = []
```

## API

The server exposes the following methods:

- `capabilities()`: Returns the server's capabilities
- `initialize(params)`: Initializes the embedding model with the specified variant
- `embed(params)`: Generates embeddings for the provided text

### Parameters

- `text`: The text to generate embeddings for
- `is_query`: Whether the text is a query (vs passage)
- `variant`: Model variant (`small`, `base`, `large`)

## Model Variants

- `small`: 384-dimensional embeddings (fastest, smaller files)
- `base`: 768-dimensional embeddings (balanced)
- `large`: 1024-dimensional embeddings (potentially most accurate, largest files)

## Requirements

- Python 3.8+
- PyTorch
- sentence-transformers
- pydantic
- jsonrpcserver 