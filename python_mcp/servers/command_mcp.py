import os
import sys
# Add the PARENT directory (python_mcp) to sys.path
script_dir = os.path.dirname(os.path.abspath(__file__))
parent_dir = os.path.dirname(script_dir)
sys.path.insert(0, parent_dir)

import subprocess
import logging
from typing import Dict, List, Optional
# Change relative import to absolute
from servers.base_server import McpBaseServer

logger = logging.getLogger(__name__)

def execute_command(args: Dict) -> Dict:
    command = args.get("command") # String: the command itself
    cmd_args = args.get("args") # List of strings: arguments
    working_dir = args.get("working_dir") # Optional string: directory to run in
    timeout_secs = args.get("timeout_secs") # Optional integer: timeout in seconds

    # --- Argument Validation ---
    if not command or not isinstance(command, str):
        raise ValueError("Missing or invalid required field: command (string)")
    if cmd_args is None:
        cmd_args = [] # Default to empty list if not provided
    elif not isinstance(cmd_args, list) or not all(isinstance(a, str) for a in cmd_args):
        raise ValueError("Invalid field: args must be a list of strings")
    if working_dir is not None and not isinstance(working_dir, str):
        raise ValueError("Invalid field: working_dir must be a string")
    if timeout_secs is not None:
        if not isinstance(timeout_secs, int) or timeout_secs <= 0:
            raise ValueError("Invalid field: timeout_secs must be a positive integer")
    # --- End Validation ---

    full_cmd = [command] + cmd_args
    logger.info(f"Executing command: {' '.join(full_cmd)}")
    if working_dir:
        logger.info(f"  in working directory: {working_dir}")
    if timeout_secs:
        logger.info(f"  with timeout: {timeout_secs} seconds")

    kwargs = {
        'stdout': subprocess.PIPE,
        'stderr': subprocess.PIPE,
        'text': True, # Decode stdout/stderr as text (usually UTF-8)
        'check': False # Don't raise CalledProcessError on non-zero exit code
    }

    if working_dir:
        # Ensure working_dir exists and is a directory before passing to subprocess
        if not os.path.isdir(working_dir):
             logger.error(f"Working directory not found or not a directory: {working_dir}")
             raise FileNotFoundError(f"Working directory not found: {working_dir}")
        kwargs['cwd'] = working_dir

    try:
        # Use subprocess.run for simpler execution and timeout handling
        result = subprocess.run(full_cmd, timeout=timeout_secs, **kwargs)
        
        logger.info(f"Command finished with exit code: {result.returncode}")
        logger.debug(f"Stdout:\n{result.stdout}")
        logger.debug(f"Stderr:\n{result.stderr}")
        
        return {
            "exit_code": result.returncode,
            "stdout": result.stdout,
            "stderr": result.stderr,
            "success": result.returncode == 0
        }

    except subprocess.TimeoutExpired as e:
        logger.warning(f"Command timed out after {timeout_secs} seconds: {' '.join(full_cmd)}")
        # Mimic Rust version's structure on timeout if possible
        # Need to check what the Rust version returns exactly
        return {
            "exit_code": None, # Or a specific code like -1?
            "stdout": "",
            "stderr": f"Command execution timed out after {timeout_secs} seconds",
            "success": False,
            "error": "TimeoutExpired" # Add specific error type
        }
    except FileNotFoundError as e:
        # Command not found
        logger.error(f"Command not found: {command}. Error: {e}")
        return {
            "exit_code": -1, # Or use specific OS error code?
            "stdout": "",
            "stderr": f"Command not found: {command}",
            "success": False,
             "error": "FileNotFoundError"
        }
    except PermissionError as e:
         # Command not executable
        logger.error(f"Permission denied executing command: {command}. Error: {e}")
        return {
            "exit_code": -1, # Or use specific OS error code?
            "stdout": "",
            "stderr": f"Permission denied executing command: {command}",
            "success": False,
            "error": "PermissionError"
        }
    except Exception as e:
        # Catch-all for other potential errors during execution
        logger.error(f"Unexpected error executing command: {e}", exc_info=True)
        return {
            "exit_code": -1,
            "stdout": "",
            "stderr": f"Unexpected error executing command: {e}",
            "success": False,
            "error": "ExecutionError"
        }

def get_environment_variable(args: Dict) -> Dict:
    var_name = args.get("name") # String: environment variable name
    if not var_name or not isinstance(var_name, str):
        raise ValueError("Missing or invalid required field: name (string)")

    logger.info(f"Getting environment variable: {var_name}")
    value = os.environ.get(var_name)

    exists = value is not None
    logger.info(f"Variable '{var_name}' exists: {exists}")
    if exists:
        # Avoid logging potentially sensitive values unless debug is on
        logger.debug(f"Value of '{var_name}': {value}") 

    return {
        "value": value, # Will be None if not found
        "exists": exists
    }

# --- Main Execution --- 

def main():
    # Define the tools provided by this server
    tools = [
        {"name": "execute_command", "description": "Executes a shell command and returns its output", "schema": None},
        {"name": "get_environment_variable", "description": "Gets the value of an environment variable", "schema": None}
    ]

    # Create the server instance
    server = McpBaseServer("command-mcp", "1.0.0", tools)

    # Register tool handlers
    server.register_tool("execute_command", execute_command)
    server.register_tool("get_environment_variable", get_environment_variable)

    # Run the server
    server.run()

if __name__ == "__main__":
    main() 