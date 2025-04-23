import os
import sys
# Add the PARENT directory (python_mcp) to sys.path
script_dir = os.path.dirname(os.path.abspath(__file__))
parent_dir = os.path.dirname(script_dir)
sys.path.insert(0, parent_dir)

import shutil
import logging
from typing import Dict, List, Optional
# Change relative import to absolute
from servers.base_server import McpBaseServer

logger = logging.getLogger(__name__)

def list_directory(args: Dict) -> Dict:
    path_str = args.get("path")
    if not path_str or not isinstance(path_str, str):
        raise ValueError("Missing or invalid required field: path (string)")
    
    logger.info(f"Listing directory: {path_str}")
    abs_path = os.path.abspath(path_str)
    logger.debug(f"Absolute path: {abs_path}")

    if not os.path.exists(abs_path):
        raise FileNotFoundError(f"Directory not found: {abs_path}")
    if not os.path.isdir(abs_path):
        raise NotADirectoryError(f"Path is not a directory: {abs_path}")

    files = []
    try:
        for entry in os.scandir(abs_path):
            try:
                is_dir = entry.is_dir()
                file_info = {
                    "name": entry.name,
                    "path": entry.path, # Keep absolute path from scandir
                    "is_directory": is_dir,
                    "size": None
                }
                
                if entry.is_file():
                    file_info["size"] = entry.stat().st_size
                    
                files.append(file_info)
            except OSError as e: # Handle permission errors for individual entries
                logger.warning(f"Could not stat entry {entry.path}: {e}")
                # Optionally add placeholder with error?
                files.append({
                    "name": entry.name,
                    "path": entry.path,
                    "is_directory": None, # Unknown
                    "size": None,
                    "error": str(e)
                })
    except OSError as e: # Handle permission error for scandir itself
         logger.error(f"Error listing directory {abs_path}: {e}")
         raise PermissionError(f"Could not list directory {abs_path}: {e}")

    logger.info(f"Found {len(files)} entries in {abs_path}")
    return {"files": files}

def read_file(args: Dict) -> Dict:
    path_str = args.get("path")
    max_size = args.get("max_size") # Should be integer or None

    if not path_str or not isinstance(path_str, str):
        raise ValueError("Missing or invalid required field: path (string)")
    if max_size is not None and not isinstance(max_size, int):
         raise ValueError("Invalid field: max_size must be an integer")

    logger.info(f"Reading file: {path_str} (max_size: {max_size})")
    abs_path = os.path.abspath(path_str)
    logger.debug(f"Absolute path: {abs_path}")

    if not os.path.exists(abs_path):
        raise FileNotFoundError(f"File not found: {abs_path}")
    if os.path.isdir(abs_path):
        # Match Rust error
        raise IsADirectoryError(f"Path is a directory, not a file: {abs_path}") 

    try:
        # Note: Python reads in text mode by default, assuming UTF-8
        # If binary files are needed, open with 'rb' and potentially return base64
        with open(abs_path, 'r', encoding='utf-8') as f:
            if max_size is not None and max_size >= 0:
                content = f.read(max_size)
                logger.info(f"Read {len(content)} bytes (up to max_size {max_size}) from {abs_path}")
            else:
                content = f.read()
                logger.info(f"Read entire file ({len(content)} bytes) from {abs_path}")
    except OSError as e:
        logger.error(f"Error reading file {abs_path}: {e}")
        raise PermissionError(f"Could not read file {abs_path}: {e}")
    except UnicodeDecodeError as e:
         logger.error(f"Error decoding file {abs_path} as UTF-8: {e}")
         raise ValueError(f"File {abs_path} is not valid UTF-8: {e}")

    return {"content": content}

def write_file(args: Dict) -> Dict:
    path_str = args.get("path")
    content = args.get("content") # Should be string
    create_dirs = args.get("create_dirs", False) # Should be boolean

    if not path_str or not isinstance(path_str, str):
        raise ValueError("Missing or invalid required field: path (string)")
    if content is None or not isinstance(content, str): # Allow empty string?
        raise ValueError("Missing or invalid required field: content (string)")
    if not isinstance(create_dirs, bool):
         raise ValueError("Invalid field: create_dirs must be a boolean")

    logger.info(f"Writing file: {path_str} (create_dirs: {create_dirs})")
    abs_path = os.path.abspath(path_str)
    logger.debug(f"Absolute path: {abs_path}")

    if os.path.isdir(abs_path):
        raise IsADirectoryError(f"Path exists and is a directory: {abs_path}")

    try:
        if create_dirs:
            dir_path = os.path.dirname(abs_path)
            if not os.path.exists(dir_path):
                 logger.info(f"Creating directory: {dir_path}")
                 os.makedirs(dir_path, exist_ok=True)
        
        # Write in text mode, assuming UTF-8
        with open(abs_path, 'w', encoding='utf-8') as f:
            f.write(content)
        logger.info(f"Successfully wrote {len(content)} characters to {abs_path}")
    except OSError as e:
        logger.error(f"Error writing file {abs_path}: {e}")
        raise PermissionError(f"Could not write file {abs_path}: {e}")

    return {"success": True}

def delete_file(args: Dict) -> Dict:
    path_str = args.get("path")
    if not path_str or not isinstance(path_str, str):
        raise ValueError("Missing or invalid required field: path (string)")

    logger.info(f"Deleting file/directory: {path_str}")
    abs_path = os.path.abspath(path_str)
    logger.debug(f"Absolute path: {abs_path}")

    if not os.path.exists(abs_path):
        # Match Rust behavior: succeed even if file doesn't exist?
        # Or raise FileNotFoundError? Rust seems to error.
        raise FileNotFoundError(f"File or directory not found: {abs_path}")

    try:
        if os.path.isdir(abs_path):
            logger.info(f"Removing directory recursively: {abs_path}")
            shutil.rmtree(abs_path)
        else:
            logger.info(f"Removing file: {abs_path}")
            os.remove(abs_path)
        logger.info(f"Successfully deleted: {abs_path}")
    except OSError as e:
        logger.error(f"Error deleting {abs_path}: {e}")
        raise PermissionError(f"Could not delete {abs_path}: {e}")

    return {"success": True}

def get_current_dir(args: Dict) -> Dict:
    # `args` is unused for this tool, but kept for consistency
    logger.info("Getting current working directory")
    try:
        cwd = os.getcwd()
        logger.info(f"Current working directory: {cwd}")
        return {"path": cwd}
    except OSError as e:
         logger.error(f"Error getting current directory: {e}")
         raise OSError(f"Could not get current working directory: {e}")

# --- Main Execution --- 

def main():
    # Define the tools provided by this server
    tools = [
        {"name": "list_directory", "description": "Lists files in a directory", "schema": None},
        {"name": "read_file", "description": "Reads the content of a file", "schema": None},
        {"name": "write_file", "description": "Writes content to a file", "schema": None},
        {"name": "delete_file", "description": "Deletes a file or directory", "schema": None},
        {"name": "get_current_dir", "description": "Gets the current working directory", "schema": None}
    ]

    # Create the server instance
    server = McpBaseServer("filesystem-mcp", "1.0.0", tools)

    # Register tool handlers
    server.register_tool("list_directory", list_directory)
    server.register_tool("read_file", read_file)
    server.register_tool("write_file", write_file)
    server.register_tool("delete_file", delete_file)
    server.register_tool("get_current_dir", get_current_dir)

    # Run the server
    server.run()

if __name__ == "__main__":
    main() 