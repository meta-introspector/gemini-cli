# Gemini Rust CLI

A command-line interface (CLI) tool written in Rust to interact with Google Gemini models from the Linux terminal.

## Features

*   Send prompts to Gemini models.
*   View the last 5 commands from your shell history for better context.
*   **In-memory conversation history** for continuous conversations in the same terminal session.
*   **Automatic conversation summarization** when token count gets too large.
*   Persistent configuration for API Key and System Prompt.
*   Configure API key via config file, environment variable, or command-line flag (for setting).
*   Special flag (`-c`) to request Linux command help. **The CLI will propose a command and ask for your confirmation before executing it.**

## Prerequisites

*   **Rust Toolchain:** Install from [https://rustup.rs/](https://rustup.rs/)
*   **Gemini API Key:** Obtain from [Google AI Studio](https://aistudio.google.com/app/apikey).
*   **Supported Shell:** Bash or Zsh (for automatic wrapper function installation).

## Installation

The easiest way to install is to use the provided installation script:

```bash
# Clone the repository
# git clone <repository_url>
# cd gemini-cli-repo

# Run the installation script from project root or parent directory
./install.sh
```

The script will:
1. Check if Rust is installed.
2. Build the release binary (`gemini-cli-bin`).
3. Install the binary to `~/.local/bin/gemini-cli-bin`.
4. **Add a wrapper function named `gemini`** to your `~/.bashrc` or `~/.zshrc`.
5. Prompt you to reload your shell configuration (e.g., `source ~/.zshrc`).

**Important:** You *must* reload your shell configuration after installation for the `gemini` command (the wrapper function) to become available.

### Manual Installation (If not using Bash/Zsh or prefer manual setup)

1. Build the binary: `cargo build --release`
2. Copy the binary: `cp target/release/gemini-cli ~/.local/bin/gemini-cli-bin`
3. Ensure `~/.local/bin` is in your PATH.
4. Manually add the following wrapper function to your shell config file:
   ```bash
   # Gemini CLI Wrapper Function Start
   # This function wraps the gemini-cli-bin to manage session environment variables
   gemini() {
       # Path to the actual binary
       local gemini_bin="$HOME/.local/bin/gemini-cli-bin"
       
       # Check if binary exists
       if [ ! -x "$gemini_bin" ]; then
           echo "Error: gemini-cli-bin not found or not executable at $gemini_bin" >&2
           return 1
       fi

       # Run the actual binary, capturing output
       local output
       # Use eval to handle potential quotes in arguments correctly
       output=$(eval "$gemini_bin \"\$@\"") 
       local exit_code=$?

       # Extract export commands from the output if the command was successful
       if [ $exit_code -eq 0 ]; then
           # Filter lines starting with # export
           local exports
           exports=$(echo "$output" | grep '^# export')

           # Execute the export commands if any were found
           if [ -n "$exports" ]; then
               # Remove the comment and execute
               eval "$(echo "$exports" | sed 's/^# export //')"
           fi
           
           # Print the output, excluding the export lines
           echo "$output" | grep -v '^# export'
       else
           # If the command failed, just print the output (likely error messages)
           echo "$output"
       fi

       return $exit_code
   }
   # Gemini CLI Wrapper Function End
   ```
5. Reload your shell configuration.

## Configuration

The CLI uses a configuration file typically located at `~/.config/gemini-cli/config.toml`.

You can manage the configuration using these flags:

*   **Set API Key:** Saves the key persistently.
    ```bash
    gemini --set-api-key YOUR_API_KEY_HERE
    ```
*   **Set System Prompt:** Saves the default instructions for the AI.
    ```bash
    gemini --set-system-prompt "You are a Rust programming expert."
    ```
*   **Show Configuration:** Displays the current settings.
    ```bash
    gemini --show-config
    ```

**Command History Context:**
By default, Gemini CLI will access your last 5 terminal commands to provide context to the AI. This helps Gemini provide more relevant responses, especially for command-related queries.

**Chat History:**
Gemini CLI maintains conversation history within a terminal session using memory only via environment variables. This allows for back-and-forth conversations.

The **`gemini` command is actually a shell function** that wraps the underlying binary (`gemini-cli-bin`). This function automatically handles the environment variables needed to maintain history across multiple calls within the same shell session.

*   To start a new conversation: `gemini --new-chat "Hello"`
*   To disable conversation history: `gemini --disable-history`
*   To enable conversation history: `gemini --enable-history`

**Automatic History Summarization:**
When a conversation gets too long (exceeding 700,000 estimated tokens), Gemini CLI will automatically summarize the conversation to reduce token usage while preserving the key context and information. This ensures that:

1. Long conversations remain manageable
2. You stay within API token limits
3. Context from earlier in the conversation is preserved in a condensed form

For long conversations across multiple commands, you may need to run the export commands shown in the output to maintain history across commands.

To see the current conversation history, token estimates, and system prompt (for debugging):
```bash
GEMINI_DEBUG=1 gemini "your prompt"
```

**API Key Precedence:**
1.  Value set in the configuration file (`~/.config/gemini-cli/config.toml`).
2.  `GEMINI_API_KEY` environment variable.

**System Prompt Precedence:**
1.  Value set in the configuration file.
2.  Default: "You are a helpful assistant."

**Note:** The `dotenv` crate is still used, so a `.env` file in the **current working directory** (where you run `gemini`) or the **project root** (during development) can set the `GEMINI_API_KEY` environment variable if it's not set globally or in the config file.

## Usage

```bash
# Configure first (if needed)
gemini --set-api-key YOUR_API_KEY_HERE
gemini --set-system-prompt "Be concise."

# Basic prompt (no flag needed)
gemini "Explain quantum physics simply"

# Continue the conversation in the same session
gemini "What's a practical application of that?"

# Start a new conversation
gemini --new-chat "How do I set up SSH keys?"

# Using command help flag
# This will ask Gemini for a command, display it, and prompt for confirmation before running.
gemini -c "list files sorted by size"

# Manage history
gemini --disable-history
gemini --enable-history

# Show current config
gemini --show-config

# Get help
gemini --help
```

## Development

Run directly using `cargo run`:

```bash
# Make sure you have a .env file in gemini-cli/ or export GEMINI_API_KEY
# Run from the workspace root (/home/james/Documents/gemini-cli)

cargo run --manifest-path gemini-cli/Cargo.toml -- "Your prompt"

cargo run --manifest-path gemini-cli/Cargo.toml -- -c "find text in files"
``` 