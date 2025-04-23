import os
import sys
# Add the PARENT directory (python_mcp) to sys.path
script_dir = os.path.dirname(os.path.abspath(__file__))
parent_dir = os.path.dirname(script_dir)
sys.path.insert(0, parent_dir)

import logging
# Change relative import to absolute
from servers.base_server import McpBaseServer

logger = logging.getLogger(__name__)

# --- Main Execution --- 

def main():
    logger.info("Starting memory-store-mcp FACADE process...")
    # Define the server name and version, but provide an EMPTY list of tools.
    # The actual tools are handled internally by mcp-hostd.
    tools = [] 

    # Create the server instance
    # It will handle the initialize/shutdown/exit handshake via the base class.
    server = McpBaseServer("memory-store-mcp", "1.0.0", tools)

    # Register NO tool handlers.
    # server.register_tool(...) <-- Do not register any tools here

    # Run the server loop (handles handshake)
    server.run()
    logger.info("memory-store-mcp FACADE process finished.")

if __name__ == "__main__":
    # This allows running the server script directly
    main() 