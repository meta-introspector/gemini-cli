# IDA Crate (`@ida`)

## Overview

The `IDA` (Internal Dialogue App) crate implements a background daemon responsible for managing persistent memory and potentially other background cognitive tasks for the main Host Application (`HAPPE`). It acts as a specialized service that enhances the context provided to the LLM and learns from interactions over time.

## Core Responsibilities

1.  **IPC Server:** Listens for requests from the `HAPPE` daemon via Inter-Process Communication (IPC).
2.  **Memory Retrieval (Pre-computation):**
    *   Receives the raw user query from `HAPPE`.
    *   Uses its MCP client to call the `Memory MCP Server` (e.g., the one wrapping LanceDB) to retrieve relevant memories based on the query (likely using semantic search via embeddings).
    *   Returns the retrieved memories (or an indication of none) back to `HAPPE` via IPC, allowing `HAPPE` to append them to the prompt.
3.  **Memory Storage (Post-computation - Asynchronous):**
    *   Receives the full conversation turn details (original query, appended memories, LLM response) asynchronously from `HAPPE` via IPC after the turn is complete.
    *   Analyzes this information to determine what new knowledge should be stored as memory.
    *   (Potentially) Generates embeddings for the new memory candidates.
    *   Uses its MCP client to call the `Memory MCP Server` to check if the candidate memory is a duplicate of existing memories (e.g., via similarity search).
    *   If the memory is novel and relevant, uses its MCP client to instruct the `Memory MCP Server` to store the new memory.
4.  **Interaction with Memory Store:** Handles all direct interaction logic (via MCP calls) with the underlying memory persistence layer (e.g., LanceDB via its MCP Server).

## Architecture

`IDA` is designed as a long-running daemon process, communicating with `HAPPE` via IPC and with the `Memory MCP Server` via MCP.

Its primary functions are triggered by IPC calls from `HAPPE`:

*   **Synchronous Request/Response for Retrieval:** When `HAPPE` gets a new query, it asks `IDA` for memories and waits for the response before proceeding.
*   **Asynchronous Notification for Storage:** After `HAPPE` completes an interaction turn, it sends the details to `IDA` and does not wait for a response, allowing `IDA` to process storage in the background.

```mermaid
sequenceDiagram
    participant HAPPE (Daemon)
    participant IDA (Daemon via IPC)
    participant MemoryMCPServer

    %% Retrieval Phase
    HAPPE->>+IDA: Send Raw Query [IPC Call]
    IDA->>+MemoryMCPServer: retrieve_memories(raw_query)
    MemoryMCPServer-->>-IDA: Return Relevant Memories
    IDA-->>-HAPPE: Send Back Memories [IPC Response]

    %% Storage Phase (Triggered Later)
    HAPPE-)+IDA: Send Full Turn Info [Async IPC Call]
    activate IDA
    IDA->>IDA: Analyze Turn Data
    IDA->>IDA: Generate Embeddings (Optional)
    IDA->>+MemoryMCPServer: check_duplicates(candidate_memory)
    MemoryMCPServer-->>-IDA: Duplication Check Result
    alt Memory is Novel
        IDA->>+MemoryMCPServer: store_memory(new_memory)
        MemoryMCPServer-->>-IDA: Ack Storage
    end
    deactivate MemoryMCPServer
    deactivate IDA

```

## Usage

This crate builds into the `ida-daemon` executable. It needs to be running for `HAPPE` to function correctly with memory capabilities. Configuration (IPC address/path, Memory MCP Server details) will likely be managed via configuration files or environment variables.

```bash
# Build
cargo build --release

# Run (Example)
./target/release/ida-daemon
```

## Dependencies

*   `tokio`: For asynchronous runtime.
*   `serde`, `serde_json`: For serialization/deserialization.
*   IPC Crate (e.g., `interprocess`): For listening to HAPPE.
*   `@mcp` or MCP client logic: For interacting with the Memory MCP Server.
*   (Potentially) Embedding generation crate.
*   (Potentially) `@core`: For shared types or utilities. 