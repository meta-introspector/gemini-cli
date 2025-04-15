# Gemini CLI Development Plan

This plan outlines the steps to address remaining compilation errors after the initial refactoring.

## Phase 1: Bug Fixing (Focus: `src/mcp/host.rs`)

1.  **Analyze `host.rs` Errors:** Systematically review the `cargo build` output for errors specific to `src/mcp/host.rs`.
2.  **Fix `self` Parameter Usage:** The helper functions `execute_tool_by_qualified_name` and `get_resource_by_qualified_name` are currently outside the `impl McpHost` block, causing the errors with `&self`. Move these functions back inside the `impl McpHost { ... }` block.
3.  **Fix Imports:** Resolve the missing imports in `host.rs`:
    *   `GetResourceParams` (needs definition or correction in `rpc.rs`)
    *   `Message` (needs definition or correction in `rpc.rs`)
    *   `LogMessageParams` (needs definition or correction in `rpc.rs`)
    *   `AsyncReadExt` (add `use tokio::io::AsyncReadExt;`)
4.  **Fix Struct Field Mismatches:**
    *   Update `ExecuteToolParams` usage in `host.rs` to use the correct fields (`tool_name`, `arguments`).
    *   Update `Tool` usage to access `parameters` instead of the non-existent `schema` field.
5.  **Implement `ActiveServer` Request Methods:** The errors indicate that `execute_tool` and `get_resource` are being called on `ActiveServer` instances, but these methods don't exist there. Implement these `async` methods within an `impl ActiveServer { ... }` block. These methods should:
    *   Take the necessary parameters (tool/resource name, arguments).
    *   Get the next request ID from the shared `AtomicU64`.
    *   Create the JSON-RPC `Request` struct.
    *   Serialize the request to JSON.
    *   Create a `oneshot` channel for the response.
    *   Store the `PendingRequest` (with the `oneshot::Sender`) in the `pending_requests` map.
    *   Send the JSON request string over the `stdin_tx` channel.
    *   `await` the result from the `oneshot::Receiver` with a timeout.
6.  **Fix `handle_response` Logic:**
    *   Correctly parse `response.id` (which is `serde_json::Value`) to get an optional `u64`. Handle cases where the ID is not a number or is missing.
    *   Adjust the `match response.result` logic, as `response.result` is an `Option<Value>`, not a `Result`. Match on the `Option` first.
7.  **Fix Async Closure (`await`):** Modify the closure used within `find_ready_server` or its usage (likely in `get_all_capabilities`) to be `async` so that `.await` can be used inside it.
8.  **Fix `Display` Errors:** Use the debug format `{:?}` for `Option<u64>` values in `log::warn!` and `log::debug!` macros, or handle the `Option` explicitly.
9.  **Fix `Send`/`Sync`/`Sized` Errors:** In `src/mcp/config.rs`, modify the error handling in `get_mcp_config_path` to ensure the returned error type (likely within the `io::Error::new` call) is `Send + Sync + 'static`. Using `Box<dyn Error + Send + Sync>` for the function's return type might be necessary if `confy`'s error isn't compatible.
10. **Clean Up Warnings:** Address all remaining `unused_imports` and `unused_variables` warnings reported by `cargo build`. 

## Task Loop Feature Implementation

### Goal
Allow the AI to persistently work on a user-defined task across multiple interactions, using available tools as needed.

### Trigger 
`gemini-cli -t "Task description"`

### Mechanism
1. The CLI application manages the loop state, but the AI controls the flow.
2. The initial task is sent to the Gemini API with system instructions that define three explicit signals:
   * `TASK_COMPLETE:` - Included in responses when the task is finished (can be at beginning or end)
   * `TASK_STUCK:` - Included in responses when the AI cannot proceed (can be at beginning or end)
   * `WAITING_FOR_USER_INPUT` - Appended to responses when user input is needed
3. The CLI interprets Gemini's responses:
   * If the response contains `TASK_COMPLETE:` or `TASK_STUCK:`, the loop terminates (with optional user confirmation).
   * If the response ends with `WAITING_FOR_USER_INPUT`, the CLI pauses, prompts the user for input, and sends it back to Gemini.
   * If the response contains a tool call, the CLI executes it (potentially with user confirmation) and sends the result back.
   * Otherwise, the CLI displays the response and immediately continues the loop without user input.
4. The loop continues until a termination signal is received or the user manually interrupts (e.g., Ctrl+C).

### System Prompt Example
```
SYSTEM: You are operating in a task loop mode. Your objective is to complete the specified task.
- If you need specific information from the user, ask your question clearly and end your response precisely with "WAITING_FOR_USER_INPUT".
- Request tool usage when necessary. Available tools are [List dynamically inserted here].
- When the task is fully completed, include "TASK_COMPLETE: " followed by a summary in your response. This can be at the beginning or end of your message.
- If you cannot complete the task, include "TASK_STUCK: " followed by the reason in your response. This can be at the beginning or end of your message.
- Otherwise, provide updates on your progress and continue working autonomously.
``` 