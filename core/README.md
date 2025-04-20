# Gemini Core Crate (`gemini-core`)

This crate provides the core functionalities for interacting with the Google Gemini API in Rust. It includes an asynchronous API client, configuration management, data structures for API communication, error handling, and support for tool calling.

## Features

*   **Asynchronous API Client**: `GeminiClient` for non-blocking communication with the Gemini API (`generateContent` endpoint) using `reqwest`.
*   **Configuration Management**: Load and save configuration (`GeminiConfig`) including API keys, model names, system prompts, and other settings via TOML files. Sensible defaults and home directory detection are included.
*   **Type-Safe API Structures**: Rust structs mirroring the Gemini API's JSON request/response schema (e.g., `GenerateContentRequest`, `GenerateContentResponse`, `Content`, `Part`, `FunctionCall`, `FunctionResponse`).
*   **Tool Calling Support**: Definitions for declaring tools (`Tool`, `FunctionDeclaration`) and handling function calls/responses within API interactions.
*   **Robust Error Handling**: A comprehensive `GeminiError` enum and `GeminiResult<T>` type built with `thiserror`.
*   **JSON-RPC Types**: Includes standard JSON-RPC structures (`Request`, `Response`, `JsonRpcError`) and specific definitions (`ServerCapabilities`, `Tool`, `Resource`), potentially for integration with language servers or other RPC-based tools.

## Modules

*   `client`: Contains the main `GeminiClient` for API interaction.
*   `config`: Provides the `GeminiConfig` struct and functions for loading/saving configuration.
*   `types`: Defines the primary data structures for Gemini API requests and responses, including content parts and tool calling elements.
*   `errors`: Defines the `GeminiError` enum and `GeminiResult<T>` type for error handling.
*   `rpc_types`: Contains JSON-RPC related structures, possibly for advanced integration scenarios.

## Installation

Add this crate to your `Cargo.toml` dependencies:

```toml
[dependencies]
gemini-core = { path = "../core" } # Adjust path as needed, or use version = "..."
```

(Note: The dependency uses workspace features, ensure your main `Cargo.toml` defines the required dependencies like `reqwest`, `serde`, `tokio`, etc.)

## Basic Usage

```rust
use gemini_core::{GeminiClient, GeminiConfig, get_default_config_file, GenerateContentRequest, Content, Part};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load configuration (uses defaults or loads from ~/.config/your_app_name/config.toml)
    // Replace "your_app_name" with the actual name used for config storage
    let config_path = get_default_config_file("your_app_name")?;
    let mut config = GeminiConfig::load_from_file(&config_path)?;

    // Ensure API key is set (e.g., from env var or loaded config)
    if config.api_key.is_none() {
        config.api_key = std::env::var("GEMINI_API_KEY").ok();
    }

    // Create the client
    let client = GeminiClient::new(config)?;

    // Example: Simple text generation
    let request = GenerateContentRequest {
        contents: vec![Content {
            parts: vec![Part::text("Explain the concept of asynchronous programming in Rust.".to_string())],
            role: Some("user".to_string()),
        }],
        system_instruction: None, // Or Some(Content { ... }) based on config
        tools: None,
        generation_config: None, // Use default generation config
    };

    match client.generate_content(request).await {
        Ok(response) => {
            match client.extract_text_from_response(&response) {
                Ok(text) => println!("Gemini Response: {}\n", text),
                Err(e) => eprintln!("Failed to extract text: {}", e),
            }
            // You can also extract function calls if tools were used:
            // let function_calls = client.extract_function_calls_from_response(&response);
            // Handle function calls...
        }
        Err(e) => eprintln!("API Error: {}", e),
    }

    // Example: Using the convenience chat method
    match client.chat("What is the capital of France?").await {
        Ok(response_text) => println!("Chat Response: {}\n", response_text),
        Err(e) => eprintln!("Chat Error: {}", e),
    }


    Ok(())
}
```

**Note**: This example assumes you have set the `GEMINI_API_KEY` environment variable or have a valid configuration file with the API key. Remember to replace `"your_app_name"` with the appropriate name for your application when handling configuration paths. 