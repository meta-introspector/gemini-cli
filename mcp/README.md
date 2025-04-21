# Gemini MCP Host Crate (`gemini-mcp`)

This crate implements the host component of the Machine Capability Protocol (MCP). It acts as a bridge between the Google Gemini API (using `gemini-core`) and various external tools or services (MCP servers) that provide specific capabilities.

The `mcp-hostd` binary included in this crate is the main daemon process.

## Features

*   **MCP Host Implementation**: Provides the `McpHost` struct which manages the lifecycle and communication with configured MCP servers.
*   **Server Discovery & Management**: Loads server configurations from `~/.config/gemini-suite/mcp_servers.json`.
*   **Multiple Transports**: Supports connecting to MCP servers via `Stdio`, `SSE` (Server-Sent Events), and `WebSocket`.
*   **Process Management (Stdio)**: Launches and manages the lifecycle of MCP servers configured to run as local processes via standard I/O.
*   **JSON-RPC Communication**: Handles MCP's JSON-RPC 2.0 based communication for initialization, tool execution (`mcp/tool/execute`), resource retrieval (`resource/get`), and standard notifications (logs, progress, cancellation).
*   **Gemini API Integration**: 
    *   Dynamically generates a system prompt for Gemini listing available tools and resources from connected MCP servers.
    *   Converts MCP tool capabilities into Gemini-compatible function declarations.
    *   Receives function calls from the Gemini API and dispatches them as `mcp/tool/execute` requests to the appropriate MCP server.
    *   Handles responses from MCP servers and formats them for the Gemini API.
*   **Capability Aggregation**: Collects capabilities (`tools`, `resources`) from all connected and initialized MCP servers.
*   **Auto-Execution Control**: Allows specific tools to be marked for automatic execution without user confirmation.
*   **Memory Broker Integration**: Implements the `McpHostInterface` trait from `gemini-memory`, allowing the memory broker to interact with MCP tools (e.g., for storing/retrieving memories).

## Built-in Servers

While the MCP architecture allows connecting to external server processes, this crate also includes the source code for several fundamental server implementations directly within the `src/servers/` directory. These implementations are not exposed as separate binaries by default but contain the logic for common capabilities:

*   **`command`**: Provides tools for executing shell commands (potentially with safety restrictions configured via arguments).
*   **`filesystem`**: Offers tools for interacting with the local filesystem (reading, writing, listing files, etc.).
*   **`memory_store`**: Implements tools for storing and retrieving key-value information, acting as a simple memory system.

These modules can be compiled into standalone server binaries or potentially integrated directly if using `gemini-mcp` as a library, depending on the application's architecture.

## Core Concepts

1.  **MCP Host (`mcp-hostd`)**: This application, run as a daemon.
2.  **MCP Servers**: Separate processes or services (potentially defined in other crates or languages) that implement the MCP specification for a specific set of tools or resources (e.g., filesystem access, command execution, database interaction).
3.  **Configuration (`mcp_servers.json`)**: A JSON file defining how the host should find and communicate with each MCP server.
4.  **Communication**: Uses JSON-RPC over the configured transport (Stdio, SSE, WebSocket).
5.  **Gemini Interaction**: The host tells Gemini what tools are available (via function declarations and system prompt) and translates Gemini's function call requests into MCP tool execution requests.

## Modules

*   `host`: Contains the core `McpHost` implementation, including server management, communication logic, and capability aggregation.
*   `servers`: Contains the Rust modules (`command`, `filesystem`, `memory_store`) implementing the built-in server logic described above. While these *can* be compiled into separate server binaries, their primary inclusion here is as part of the `gemini-mcp` library's source.
*   `gemini`: Handles the translation layer between MCP capabilities/calls and Gemini function declarations/calls.
*   `config`: Defines the `McpServerConfig` structure and logic for loading `mcp_servers.json`.
*   `rpc`: Defines MCP-specific JSON-RPC message structures (`InitializeParams`, `ExecuteToolParams`, etc.).
*   `ipc`: Contains utilities for inter-process communication, particularly for Stdio transport.
*   `bin/mcp-hostd.rs`: The source code for the runnable `mcp-hostd` daemon.

## Usage

This crate is primarily intended to be used via the `mcp-hostd` binary. 

1.  **Configure Servers**: Create or modify `~/.config/gemini-suite/mcp_servers.json` to define the MCP servers you want the host to connect to.

    ```json
    [
      {
        "name": "filesystem",
        "enabled": true,
        "transport": "stdio",
        "command": ["path/to/mcp-filesystem-server"], // Command to run the server
        "args": [],
        "env": {},
        "auto_execute": ["filesystem/readFile", "filesystem/writeFile"]
      },
      {
        "name": "command_executor",
        "enabled": true,
        "transport": "stdio",
        "command": ["path/to/mcp-command-server"],
        "args": ["--allow-commands", "ls,cat,echo"],
        "env": {},
        "auto_execute": [] // No auto-execution for commands by default
      }
      // Add configurations for other servers (SSE, WebSocket)
    ]
    ```

2.  **Run the Host Daemon**: Start the `mcp-hostd` binary.

    ```bash
    # Build the binary first if needed
    # cargo build --package gemini-mcp --bin mcp-hostd
    
    # Run the daemon (likely in the background)
    target/debug/mcp-hostd &
    ```

3.  **Interact via Gemini**: When an application using `gemini-core` interacts with the Gemini API (assuming it's configured to use the system prompt generated by the host), the model will be aware of the tools provided by the connected MCP servers and can issue function calls, which the host will route accordingly.

## Development

To use `gemini-mcp` as a library (e.g., to embed the host logic directly into another application), add it to your `Cargo.toml`:

```toml
[dependencies]
gemini-mcp = { path = "../mcp" } # Adjust path as needed, or use version = "..."
```

Then, you can instantiate and use the `McpHost`:

```rust
use gemini_mcp::{McpHost, load_mcp_servers};
use gemini_core::{GeminiClient, GeminiConfig, GenerateContentRequest, Content, Part, Tool as GeminiTool};
use gemini_mcp::{build_mcp_system_prompt, generate_gemini_function_declarations, process_function_call};
use serde_json::Value;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load MCP server configurations
    let mcp_configs = load_mcp_servers().expect("Failed to load MCP server configs");

    // Initialize the MCP Host
    let mcp_host = McpHost::new(mcp_configs).await?;

    // Get capabilities from MCP Host
    let capabilities = mcp_host.get_all_capabilities().await;
    let tools = capabilities.tools;
    let resources = capabilities.resources;

    // --- Configure Gemini Client --- 
    // (Load GeminiConfig, create GeminiClient - similar to core/README.md example)
    let gemini_config = GeminiConfig::default(); // Replace with actual config loading
    let gemini_client = GeminiClient::new(gemini_config)?; // Ensure API key is set

    // --- Build System Prompt and Function Declarations ---
    let mcp_prompt = build_mcp_system_prompt(&tools, &resources);
    let system_instruction = Content {
        parts: vec![Part::text(mcp_prompt)],
        role: Some("system".to_string()),
    };

    let function_declarations = generate_gemini_function_declarations(&tools);
    let gemini_tools = function_declarations.map(|decls| vec![GeminiTool { function_declarations: decls }]);

    // --- Make API Call with Tools ---
    let user_content = Content {
        parts: vec![Part::text("Read the first 10 lines of /etc/passwd using the filesystem tool".to_string())],
        role: Some("user".to_string()),
    };

    let request = GenerateContentRequest {
        contents: vec![user_content],
        system_instruction: Some(system_instruction),
        tools: gemini_tools,
        generation_config: None,
    };

    match gemini_client.generate_content(request).await {
        Ok(response) => {
            println!("Raw Gemini Response: {:?}", response);
            
            let function_calls = gemini_client.extract_function_calls_from_response(&response);
            
            if !function_calls.is_empty() {
                println!("\nDetected Function Calls:");
                let mut function_responses = Vec::new();

                for call in function_calls {
                     println!("  - Name: {}, Args: {}", call.name, call.arguments);
                    
                     // Convert Gemini function name back to MCP tool name if needed (e.g., dot to slash)
                     // let mcp_tool_name = call.name.replace(".", "/"); 
                     // Determine server_name based on tool_name prefix or config lookup
                     let server_name = "filesystem"; // Example: determine this dynamically

                     match mcp_host.execute_tool(server_name, &call.name, call.arguments).await {
                         Ok(result) => {
                             println!("  - Tool Result: {}", result);
                             function_responses.push(Part::function_response(call.name, result));
                         }
                         Err(e) => {
                             eprintln!("  - Tool Error: {}", e);
                             // Optionally send back an error response to Gemini
                             let error_response = json!({ "error": e });
                             function_responses.push(Part::function_response(call.name, error_response));
                         }
                     }
                }

                // TODO: Send function responses back to Gemini in a subsequent API call
                // let response_content = Content {
                //     parts: function_responses,
                //     role: Some("function".to_string()), // Or "tool" depending on API version
                // };
                // ... build new request with response_content ...
                // ... make another gemini_client.generate_content call ...

            } else {
                // Handle regular text response
                match gemini_client.extract_text_from_response(&response) {
                     Ok(text) => println!("\nGemini Text Response: {}\n", text),
                     Err(e) => eprintln!("Failed to extract text: {}", e),
                 }
            }
        }
        Err(e) => eprintln!("API Error: {}", e),
    }
    
    // Shutdown MCP Host gracefully
    mcp_host.shutdown().await;

    Ok(())
} 