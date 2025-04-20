# IPC Crate (`@ipc`)

## Overview

This crate centralizes the definitions for Inter-Process Communication (IPC) messages used between different components of the Gemini Rust Suite. Its primary purpose is to provide standardized data structures for requests and responses, ensuring consistent communication protocols across the daemons and potential future clients.

## Core Responsibilities

1.  **Define IPC Message Structures:** Provides the Rust structs and enums for messages passed between different processes.
2.  **Standardize Communication:** Ensures that components like `mcp-hostd`, `HAPPE`, and `IDA` use a common language when communicating via IPC.

## Modules

This crate is organized into modules based on the communicating parties:

*   **`daemon_messages`**: Defines the messages passed between a client (like `gemini-cli`) and the `mcp-hostd` daemon. This includes:
    *   `DaemonRequest`: Enums representing requests like `GetCapabilities`, `ExecuteTool`, `GenerateEmbedding`, `GetBrokerCapabilities`.
    *   `DaemonResponse`: Struct containing status and a payload (`DaemonResult` or `DaemonError`).
    *   Associated result, error, and status types.

*   **`internal_messages`**: Defines the messages passed specifically between the `HAPPE` daemon and the `IDA` daemon. This includes:
    *   `InternalMessage`: Enum representing messages like `GetMemoriesRequest`, `GetMemoriesResponse`, `StoreTurnRequest`.
    *   Associated data structures like `MemoryItem`.

## Usage

This crate primarily provides type definitions. The actual implementation of IPC mechanisms (e.g., using Unix domain sockets, serialization/deserialization logic) resides within the crates that use these definitions:

*   `@mcp` (specifically `mcp-hostd` binary): Uses `daemon_messages` for its IPC server logic.
*   `@HAPPE`: Uses `internal_messages` to communicate with `IDA`.
*   `@IDA`: Uses `internal_messages` to communicate with `HAPPE`.
*   `@cli` (potentially): Would use `daemon_messages` to communicate with `mcp-hostd`.

## Dependencies

*   `serde`, `serde_json`: For serialization and deserialization of messages.
*   `gemini_core`: For shared types like `ServerCapabilities` used within some messages.
*   (Potentially `chrono` if timestamps are used directly in message structs). 