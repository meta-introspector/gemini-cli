e# Gemini CLI: Interactive Mode (`-i`) and Task Mode (`-t`) Analysis

This document outlines the functionality of the interactive (`-i`) and task (`-t`) modes in the Gemini CLI application.

## 1. Default Mode (No `-i` or `-t`)

When run with just a prompt (e.g., `gemini "explain rust lifetimes"`), the CLI performs a single interaction:

1.  **Load History:** Optionally loads previous conversation history if `save_history` is enabled and a session ID is present (either from a previous run or the `GEMINI_SESSION_ID` environment variable). History is disabled if `--disable-history` is used. A new chat context is used if `--new-chat` is passed.
2.  **Prepare Prompt:** Formats the user's prompt. If the `-c` (`--command-help`) flag is used, it prepends "Provide the Linux command for: " to the prompt.
3.  **MCP Integration (Tools):**
    *   Checks if an MCP (Multi-Capability Provider) host is running and configured.
    *   If available, it retrieves the capabilities (tools and resources) announced by the connected MCP servers (e.g., filesystem server, command server).
    *   It translates these capabilities into `FunctionDeclaration` objects compatible with the Gemini API's `tools` parameter. This allows the model to request actions like reading files or executing commands.
4.  **API Call:** Sends the formatted prompt, conversation history, system prompt, and available tool declarations to the Gemini API.
5.  **Tool Execution Handling:**
    *   If the API response includes a request to call a function (tool), the CLI identifies the target MCP server and tool name (e.g., `filesystem/read_file`).
    *   It forwards the tool name and arguments to the `McpHost`, which routes the request to the appropriate server (running as a separate process or integrated).
    *   **Crucially, for potentially sensitive tools like `execute_command`, user confirmation is required by default.** The user sees the requested command and must approve (`y`es), deny (`n`o), or always allow (`a`) for the current session.
    *   The result (or error) from the tool execution is captured.
6.  **Follow-up API Call (If Tools Used):** If tools were executed, their results are sent back to the Gemini API in a follow-up request, allowing the model to generate a final response based on the tool output.
7.  **Display Response:** Prints the final text response from the Gemini API to the terminal, formatting it nicely (e.g., rendering markdown).
8.  **Save History:** Appends the user prompt and the final assistant response to the chat history file (if enabled).

## 2. Interactive Mode (`-i`)

Invoked using `gemini -i`. This mode provides a persistent chat session.

1.  **Load History:** Loads the history for the current session, similar to the default mode.
2.  **REPL Loop:** Enters a Read-Eval-Print Loop:
    *   **Read:** Uses the `rustyline` library to provide a user-friendly input prompt with history (up/down arrows) and editing capabilities.
    *   **Eval:**
        *   **Commands:** Checks if the input starts with `/` (e.g., `/quit`, `/new`, `/history`, `/clear`, `/save`, `/load`, `/help`). These commands manage the chat session itself.
        *   **Prompt Processing:** If it's not a command, the input is treated as a user prompt. It then follows a similar flow to the **Default Mode**'s steps 3-7 (MCP Integration, API Call, Tool Handling, Display Response) but *within* the loop, continuously updating the session's chat history in memory.
    *   **Print:** Displays the assistant's response.
    *   **Loop:** Continues until the user enters `/quit`.
3.  **Save History:** Saves the complete session history to the file upon exiting (if enabled).

Key difference: Maintains context across multiple turns within the same execution, allowing for conversational follow-ups. Still leverages MCP tools.

## 3. Task Mode (`-t`)

Invoked using `gemini -t "your task description"`. This mode attempts to let the AI autonomously complete a multi-step task.

1.  **Load History:** Loads history for the session.
2.  **Initialization:** The initial prompt sent to the model includes the user's task description and encourages the use of available tools.
3.  **Autonomous Loop:** Enters a loop that runs for a maximum number of iterations (e.g., 10):
    *   **API Call (Planning/Action):** Sends the current history (including previous steps, tool results) to the Gemini API. The prompt explicitly asks the model to determine the *next step* or *action* needed to progress on the task, heavily encouraging function/tool use.
    *   **Tool Execution:**
        *   If the model requests a tool call (e.g., `command/execute_command`), the CLI intercepts it.
        *   **User Confirmation:** As in default mode, potentially dangerous actions like command execution require user confirmation (`y/n/a`). This is a critical safety/oversight mechanism.
        *   The tool is executed via the `McpHost`.
    *   **API Call (Observation):** Sends the tool execution results back to the model.
    *   **Model Response:** The model processes the tool results and outputs its next thought, action, or a final result.
    *   **Display:** Prints the model's response/action for the user to observe.
    *   **Completion Check:** Checks if the model's output indicates task completion (e.g., contains "Task Completed").
    *   **Loop/Exit:** Continues the loop or exits if the task is complete or the maximum iteration limit is reached.
4.  **Save History:** Saves the full sequence of steps, tool calls, and results (if enabled).

Key difference: Designed for the AI to work iteratively towards a goal, using tools to interact with the environment, with user oversight for command execution.

## 4. `-i` and `-t` Combined

When both the `-i` (interactive mode) and `-t` (task mode) flags are provided, the CLI enters a special "interactive task mode" that combines the benefits of both approaches. This mode allows the AI to work on a specific task while enabling the user to provide guidance, feedback, or course corrections throughout the process.

The implementation:

1.  **Initialization:**
    *   The task description is incorporated into the system prompt, instructing the AI to focus on completing that specific task.
    *   A special initial message is added to the chat history, asking the AI to analyze the task and create a step-by-step plan.
    *   The user is informed that they're entering interactive task mode and can guide the AI's work.

2.  **Interactive Task Flow:**
    *   The AI begins by analyzing the provided task and outlining a plan.
    *   The user can provide guidance, answer questions, or redirect the AI at any point.
    *   The AI can use available tools (like filesystem or command execution) to make progress on the task.
    *   The standard interactive chat commands (like `/quit`, `/help`) remain available.

3.  **Benefits:**
    *   **User Oversight:** Provides more user control than pure task mode, allowing for course correction if the AI misunderstands or takes a wrong approach.
    *   **Semi-Autonomous Operation:** The AI can still make task progress by utilizing tools, rather than requiring the user to explicitly handle each step.
    *   **Collaborative Problem-Solving:** Creates a more balanced interaction where the AI handles routine implementation details while the user provides high-level direction.

Example usage:
```bash
gemini -i -t "Create a simple React component that displays a counter with increment and decrement buttons"
```

The AI would start by outlining the steps to create the React component, then begin implementing it, potentially using filesystem tools to create the necessary files, while allowing the user to provide feedback or make adjustments throughout the process.

## 5. Model-Led Interactive Mode

This enhancement to the standard interactive (`-i`) mode allows the AI model to continue the conversation proactively after its response, without requiring explicit user prompts at every turn.

**Implemented Functionality:**

1.  **Signal for User Input:**
    *   The model is instructed via the system prompt to end its message with the exact phrase `AWAITING_USER_RESPONSE` when it specifically needs user input to proceed.
    *   If this signal is not present, the system automatically continues the conversation with a "Continue." prompt.
    *   This signal is removed from the displayed output to keep the conversation natural.

2.  **Auto-Continuation:**
    *   When the model doesn't signal for user input, it is immediately re-prompted to continue its line of reasoning or explanation.
    *   A counter tracks consecutive model turns to prevent potential infinite loops, with a default limit of 5 consecutive turns.
    *   When the limit is reached, the system automatically pauses and prompts the user for input.

3.  **User Control:**
    *   The user can interrupt the model's sequence at any time by entering a new prompt or command.
    *   Traditional commands like `/quit` still function normally.

4.  **Tool Integration:**
    *   The model can still use tools (filesystem, command execution, etc.) during its autonomous turns.
    *   After tool execution, the model can either continue autonomously or signal for user input.

**Benefits:**

*   **Extended Reasoning:** Allows the model to develop complex ideas across multiple turns without interruption.
*   **Multi-Step Explanations:** Enables the model to break down detailed explanations into logical segments.
*   **Autonomous Problem Solving:** The model can work through a problem step-by-step, using both reasoning and tools.
*   **Natural Flow:** Creates a more conversational experience where the model can elaborate when needed and ask for guidance when appropriate.

This enhancement is particularly useful for complex tasks like debugging, step-by-step tutorials, or detailed explanations where forcing the model to compress everything into a single response would be limiting. 

## 6. Planned Enhancements

Based on analysis of the current implementation, the following enhancements are planned to improve the robustness, usability, and flexibility of Gemini CLI's interactive and task modes:

### 6.1 Signal Mechanism Improvements

1. **Structured Signaling:**
   * [x] Replace text-based `AWAITING_USER_RESPONSE` with a more robust JSON-based control token mechanism
   * [x] Allow multiple signal types (e.g., `NEED_INPUT`, `TASK_COMPLETE`, `PROGRESS_UPDATE`)
   * [x] Implementation: Update system prompts and response processing to handle structured signals

2. **Progress Tracking:**
   * [x] Enable the model to report task progress percentage
   * [x] Add structured format for reporting subtask completion
   * [x] Implementation: Enhance system prompt with progress reporting instructions

### 6.2 User Experience Enhancements

1. **Visual Indicators:**
   * [x] Add clear status line showing current mode (interactive, task, combined)
   * [x] Use different colored prompts to indicate auto-continue vs. waiting for input
   * [x] Implementation: Add terminal coloring and status indicators in CLI output

2. **Command Accessibility:**
   * [x] Allow special commands during auto-continue cycles
   * [x] Add `/interrupt` command to force model to pause and await input
   * [x] Implementation: Add keyboard interrupt handler and command processing

3. **Configuration Options:**
   * [x] Add CLI flags to customize behavior:
     * [x] `--max-consecutive-turns=N` (default: 5)
     * [x] `--auto-continue=(on|off)` (default: on)
     * [x] `--progress-reporting=(on|off)` (default: on)
   * [x] Implementation: Update CLI args parser and pass values to appropriate functions

### 6.3 Code Architecture Improvements

1. **Tool Execution Refactoring:**
   * [x] Extract duplicated tool execution logic into shared utility functions
   * [x] Standardize error handling and recovery mechanisms
   * [x] Implementation: Create `execute_tool_with_confirmation()` in utils module

2. **History Management:**
   * [x] Standardize history summarization across all modes
   * [x] Implement improved token tracking and management
   * [x] Implementation: Update history module with consistent summarization logic

3. **Error Recovery:**
   * [x] Add retry mechanisms for failed API calls
   * [x] Implement graceful degradation for tool failures
   * [x] Implementation: Add resilience patterns with exponential backoff

### 6.4 Security and Reliability

1. **Tool Confirmation Standardization:**
   * [x] Ensure consistent security prompts across all modes
   * [x] Add configurable security levels for different tool types
   * [x] Implementation: Centralize confirmation logic in a dedicated module

2. **Execution Limits:**
   * [x] Add timeouts for tool execution
   * [x] Implement total execution time limits for task mode
   * [x] Implementation: Add timeout parameters to tool execution functions

### 6.5 Implementation Status

All planned enhancements have been successfully implemented across the three phases:

1. **Phase 1 (Short-term):** ✓
   * [x] Command accessibility improvements
   * [x] Visual indicators for mode status
   * [x] Tool execution refactoring

2. **Phase 2 (Medium-term):** ✓
   * [x] Configuration options via CLI flags
   * [x] Structured signaling mechanism
   * [x] Standardized history management

3. **Phase 3 (Long-term):** ✓
   * [x] Advanced progress tracking
   * [x] Enhanced error recovery
   * [x] Security and reliability features

The implementation of these enhancements has significantly improved the Gemini CLI's robustness, user experience, and capabilities. The tool now provides a more intuitive interface, better error handling, and enhanced security while maintaining flexibility for various use cases.

### 6.6 Future Directions

While the planned enhancements have been completed, potential future improvements could include:

1. **Plugin System:**
   * [x] Develop an extensible plugin architecture for custom tools and capabilities
   * [x] Create a standardized API for third-party integrations
   * [ ] Implement a package management system for plugins

2. **Advanced Context Management:**
   * **Storage & Scalability (LanceDB Integration):**
     * [x] Add `lancedb` crate as a dependency to `memory/Cargo.toml`.
     * [x] Replace the JSON file storage in the `memory` crate with LanceDB (embedded mode).
     * [x] Define a LanceDB table schema within `MemoryStore` mirroring the `Memory` struct, including a vector column.
     * [x] Initialize the LanceDB connection (`lancedb::connect`) within `MemoryStore::load` (or a new constructor).
     * [x] Refactor `MemoryStore` CRUD methods (`add_memory`, `update_memory`, `get_by_key`, `get_by_tag`, `get_all_memories`, `delete_by_key`) to interact with the LanceDB table using the Rust SDK.
   * **Semantic Retrieval (LanceDB + E5 Integration):**
     * [x] Implement embedding generation logic via a Python-based MCP server:
       * [x] Create a new Python project for the MCP server (e.g., `mcp_embedding_server`).
       * [x] Add dependencies: `lancedb`, `sentence-transformers`, `torch`, `pydantic` (minimal required for JSON-RPC handling over stdio).
       * [x] Implement MCP server logic using **`stdio` transport** (read JSON-RPC from stdin, write to stdout).
       * [x] Expose an `embed` tool via the server.
       * [x] The `embed` tool should accept text and model variant (large/base/small) as input.
       * [x] **Load the selected `sentence-transformers` E5 model on server startup** for consistent performance.
       * [x] Ensure the server correctly prefixes inputs with `"query: "` or `"passage: "` before encoding.
       * [x] Return the generated embedding vector.
     * [x] Add configuration (CLI flag or config file) to select the E5 model variant (large/base/small) to be passed to the MCP server.
     * [x] Call the `embed` tool on the MCP server via the `memory::broker` interface to generate embeddings for memory `value` fields before storing them in LanceDB.
     * [x] Store the generated embeddings in the designated vector column of the LanceDB table during `add_memory` and `update_memory`.
     * [x] Implement vector similarity search using `table.search(query_vector).limit(top_k).execute()` within `MemoryStore`.
     * [x] Add a new retrieval method like `get_semantically_similar(query_text: &str, top_k: usize) -> Result<Vec<Memory>, Error>`:
       * [x] This method should take raw query text.
       * [x] Prefix the query text with `"query: "`.
       * [x] Call the MCP server's `embed` tool to generate the embedding for the prefixed query using the selected E5 model.
       * [x] Perform the LanceDB vector search using the generated query embedding.
       * [x] Return the retrieved `Memory` objects.
   * **Enhanced Retrieval Strategies:**
     * [x] Implement hybrid search combining semantic similarity and keyword/tag matching. (`search_memories`)
     * [x] Add time-based filtering options (e.g., `get_recent(duration: Duration)`, `get_in_range(start: u64, end: u64)`).
     * [x] Develop an interface for complex queries combining multiple filters (tags, time, semantic, keywords) (`search_memories`).
     * [x] Consider allowing user configuration of retrieval strategy parameters (e.g., weighting). (Implemented via RRF hybrid search)
   * **Token Management & Summarization:**
     * [x] Implement accurate token counting for memory content (`tiktoken_rs`).
     * [x] Develop strategies for selecting/summarizing memories to fit context windows, prioritizing by relevance and recency. (Implemented as a configurable adapter layer in `memory::contextual` that allows callers to use retrieved data with token budget constraints)
     * [x] Re-implement summarization logic, potentially as an MCP tool callable via the `memory::broker` interface. (Implemented via the `memory_summarization` MCP tool with configurable summarization strategies)
   * **Context Visualization & Navigation Support:**
     * [x] Enhance the `Memory` struct with additional metadata (e.g., source session ID, related entities, confidence score).
     * [x] Explore adding relationship tracking between memories. (Fully implemented via expanded `related_keys` field with bidirectional relationship types)
     * [x] Provide methods in `MemoryStore` to export data suitable for graph visualization or advanced analysis (e.g., `export_all_memories_json`).
     * [x] Add a web-based visualization interface for exploring memory connections (Implemented using D3.js with the `/memory visualize` command)
     * [x] Implement memory graph navigation commands in CLI (Added `/memory graph` command with path-finding between related memories)

3. **Multimodal Interactions:** (Detailed Plan)

   Assume Python-based MCP servers communicating via `stdio`.

   * **Image Modality (`mcp_image_server.py`)**
     * **Goal:** Handle image generation and analysis requests.
     * **A. MCP Tool: `image_generation`**
       * [ ] **Sub-task 1: Define Tool Schema:** Inputs (`prompt`, `output_path`, `return_format`, `model_preference`), Outputs (`image_data`, `format`, `message`).
       * [ ] **Sub-task 2: Research & Select Backend:** Prioritize Vertex AI (Imagen) via `google-cloud-aiplatform`, consider Stability AI (API `stability-sdk` or local `diffusers`).
       * [ ] **Sub-task 3: Implement Server Logic:** `stdio` loop, parse request, handle auth (env vars), call backend API, process image response (URL/base64/save), format MCP response.
       * [ ] **Sub-task 4: Implement Error Handling:** API errors, network issues, invalid params, file errors.
       * [ ] **Sub-task 5: Configuration:** Define & document required env vars (e.g., `GOOGLE_APPLICATION_CREDENTIALS`, `STABILITY_API_KEY`).
       * [ ] **Sub-task 6: Testing:** Unit & integration tests.
     * **B. MCP Tool: `image_analysis`**
       * [ ] **Sub-task 1: Define Tool Schema:** Inputs (`prompt`, `image_input`, `input_type`), Outputs (`analysis_text`, `message`).
       * [ ] **Sub-task 2: Research & Select Backend:** Prioritize Gemini Pro Vision (via `google-generativeai` or `google-cloud-aiplatform`).
       * [ ] **Sub-task 3: Implement Server Logic:** Extend server, parse request, handle auth, load image data (path/URL/base64), call Gemini Vision API, format MCP response.
       * [ ] **Sub-task 4: Implement Error Handling:** API errors, invalid image inputs, network issues.
       * [ ] **Sub-task 5: Configuration:** Define & document required env vars.
       * [ ] **Sub-task 6: Testing:** Unit & integration tests.

   * **Audio Modality (`mcp_audio_server.py`)**
     * **Goal:** Handle STT and TTS, focusing on low-latency options.
     * **A. MCP Tool: `audio_transcribe` (STT)**
       * [ ] **Sub-task 1: Define Tool Schema:** Inputs (`audio_input`, `input_type`, `language_code`, `real_time`), Outputs (`transcribed_text`, `message`).
       * [ ] **Sub-task 2: Research & Select Backend:** Prioritize Google Cloud Speech-to-Text (Streaming via `google-cloud-speech`) for real-time. Offer `faster-whisper` as offline/local alternative.
       * [ ] **Sub-task 3: Implement Server Logic:** `stdio` loop, parse request, handle auth. Implement Cloud STT streaming logic OR call local `faster-whisper`. Format MCP response.
       * [ ] **Sub-task 4: Implement Error Handling:** API/file/streaming errors, invalid formats.
       * [ ] **Sub-task 5: Configuration:** Define env vars for cloud keys, model paths for local.
       * [ ] **Sub-task 6: Testing:** Unit & integration tests, specific streaming tests.
     * **B. MCP Tool: `audio_speak` (TTS)**
       * [ ] **Sub-task 1: Define Tool Schema:** Inputs (`text_to_speak`, `output_path`, `return_format`, `language_code`, `voice_name`, `real_time`), Outputs (`audio_data`, `format`, `message`).
       * [ ] **Sub-task 2: Research & Select Backend:** Prioritize Google Cloud TTS (`google-cloud-texttospeech`) or ElevenLabs (`elevenlabs`) for quality/streaming. Offer Piper TTS (local via executable/bindings) as fast local alternative.
       * [ ] **Sub-task 3: Implement Server Logic:** `stdio` loop, parse request, handle auth. Call selected backend, handle streaming output if applicable. Format MCP response.
       * [ ] **Sub-task 4: Implement Error Handling:** API errors, invalid text, file saving errors.
       * [ ] **Sub-task 5: Configuration:** Define env vars for cloud keys, model paths for Piper.
       * [ ] **Sub-task 6: Testing:** Unit & integration tests, streaming playback tests.

   * **Document Modality (`mcp_document_server.py`)**
     * **Goal:** Handle parsing, extraction, potentially summarization/querying.
     * **A. MCP Tool: `document_extract`**
       * [ ] **Sub-task 1: Define Tool Schema:** Inputs (`document_input`, `input_type`, `output_format`, `extract_metadata`), Outputs (`extracted_content`, `metadata`, `format`, `message`).
       * [ ] **Sub-task 2: Research & Select Backend:** Prioritize `PyMuPDF` (PDF), `python-docx` (DOCX). Consider `pypandoc` (requires `pandoc` executable) for broader text extraction, `unstructured-io` for advanced layout-aware parsing.
       * [ ] **Sub-task 3: Implement Server Logic:** `stdio` loop, parse request, determine file type, call appropriate library, format output, format MCP response.
       * [ ] **Sub-task 4: Implement Error Handling:** File not found, unsupported formats, parsing errors.
       * [ ] **Sub-task 5: Configuration:** Library installation (potentially `pandoc` PATH setup).
       * [ ] **Sub-task 6: Testing:** Unit & integration tests with sample docs.
     * **B. MCP Tools: `document_summarize` / `document_query` (Recommend Host Implementation)**
       * [ ] **Sub-task 1: Define Tool Schema:** Summarize (Inputs: `document_input`, `input_type`, `summary_length`, `focus_topic`; Outputs: `summary`). Query (Inputs: `document_input`, `input_type`, `query`; Outputs: `answer`, `relevant_snippets`).
       * [ ] **Sub-task 2: Design Approach:** Recommend implementing these in the *Host CLI*, using the `document_extract` MCP tool. An MCP server implementation is complex, requiring calls *back* to the Host's LLM (via `Sampling` or custom mechanism) and chunking logic.
       * [ ] **Sub-task 3-6 (If MCP Server):** Implement extraction, chunking, prompt formulation, host LLM request, result processing, error handling, testing (highly dependent on Host capabilities).
     * **C. MCP Tool: `document_convert` (Optional/Advanced)**
       * [ ] **Sub-task 1: Define Tool Schema:** Inputs (`document_input`, `input_type`, `target_format`, `output_path`), Outputs (`output_path`).
       * [ ] **Sub-task 2: Research & Select Backend:** `pypandoc` (requires `pandoc` executable).
       * [ ] **Sub-task 3-6:** Implement server logic using `pypandoc`, error handling, configuration (ensure `pandoc` in PATH), testing.

These future directions would further expand the utility and flexibility of the Gemini CLI, making it an even more powerful tool for AI-assisted productivity. 