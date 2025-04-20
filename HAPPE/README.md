# HAPPE Crate (`@happe`)

## Overview

The `HAPPE` (Host Application Environment) crate implements the core host daemon responsible for managing interactions between the user, the main Large Language Model (LLM), and other system components like the Internal Dialogue App (`IDA`) and MCP Servers.

It acts as the primary execution environment, orchestrating the flow of information and commands based on user requests and LLM responses.

## Core Responsibilities

1.  **User/Client Interface:** (Future) Provides the primary interface for user interaction, potentially through various means like APIs, WebSockets, or other front-ends (initially replacing the direct CLI usage).
2.  **LLM Interaction:** Manages the connection and communication with the main LLM, sending constructed prompts and receiving generated responses.
3.  **IDA Communication (IPC Client):**
    *   Connects to the `IDA` daemon via Inter-Process Communication (IPC).
    *   Sends the user's raw query to `IDA` before contacting the LLM to allow `IDA` to retrieve relevant memories.
    *   Receives retrieved memories (or none) from `IDA`.
    *   Constructs the final prompt for the LLM by **appending** the received memories to the **original user query**.
    *   Asynchronously sends the complete conversation turn (original query, appended memories, LLM response) back to `IDA` for analysis and storage after the LLM interaction is complete.
4.  **MCP Tool Execution (LLM-Initiated):** If the main LLM decides to use an MCP Tool during its response generation, `HAPPE` is responsible for invoking the appropriate MCP Server (likely via the `@mcp` crate or direct MCP client logic) and returning the result to the LLM.
5.  **State Management:** Manages the immediate state required for the current interaction turn.

## Architecture

`HAPPE` is designed as a long-running daemon process. It communicates with `IDA` via IPC. Its primary workflow involves:

1.  Receiving a user query.
2.  Querying `IDA` for contextual memories.
3.  Constructing the LLM prompt.
4.  Executing the LLM call.
5.  Handling any MCP tool calls requested by the LLM during generation.
6.  Returning the final response to the user/client.
7.  Asynchronously informing `IDA` about the completed turn for learning/memory storage.

```mermaid
sequenceDiagram
    participant User/Client
    participant HAPPE (Daemon)
    participant IDA (Daemon via IPC)
    participant MemoryMCPServer
    participant OtherMCPServers
    participant MainLLM

    User/Client->>+HAPPE: Sends Raw Query
    HAPPE->>+IDA: Send Raw Query [IPC Call]
    IDA->>+MemoryMCPServer: retrieve_memories()
    MemoryMCPServer-->>-IDA: Return Memories
    IDA-->>-HAPPE: Send Back Memories [IPC Response]
    HAPPE->>HAPPE: Construct Prompt = Query + Memories
    HAPPE->>+MainLLM: Send Prompt
    alt LLM needs MCP Tool
        MainLLM-->>HAPPE: Request Tool X
        HAPPE->>+OtherMCPServers: Execute Tool X
        OtherMCPServers-->>-HAPPE: Tool Result
        HAPPE-->>MainLLM: Provide Tool Result
    end
    MainLLM-->>-HAPPE: Receive Final LLM Response
    HAPPE-->>User/Client: Display Response
    HAPPE-)+IDA: Send Full Turn [Async IPC Call]
    IDA->>IDA: Process Turn for Storage
    deactivate HAPPE
    activate IDA
    IDA->>+MemoryMCPServer: store_memory()
    MemoryMCPServer-->>-IDA: Ack
    deactivate MemoryMCPServer
    deactivate IDA
```

## Usage

This crate builds into the `happe-daemon` executable. Configuration details (LLM endpoints, IDA IPC address, MCP server details) will likely be managed via configuration files or environment variables.

```bash
# Build
cargo build --release

# Run (Example)
./target/release/happe-daemon
```

## Dependencies

*   `tokio`: For asynchronous runtime.
*   `serde`, `serde_json`: For serialization/deserialization.
*   IPC Crate (e.g., `interprocess`): For communication with IDA.
*   LLM Client Crate: For interacting with the main LLM.
*   (Potentially) `@mcp` or MCP client logic: For handling LLM-initiated tool calls.
*   (Potentially) `@core`: For shared types or utilities. 