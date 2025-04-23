#!/usr/bin/env python3
from __future__ import annotations

# Standard import block
import abc
import argparse
import enum
import importlib.metadata
import io
import json
import logging
import os
import signal
import sys
import textwrap
import traceback
import time
from typing import Any, Callable, Dict, List, Optional, Union
import re

# ===== Setup unbuffered binary I/O for stdin/stdout =====
# This is CRITICAL for JSON-RPC over stdio to work properly
try:
    # Configure stdin/stdout for binary mode
    sys.stderr.write("SETUP: Configuring stdin/stdout for binary mode...\n")
    sys.stderr.flush()
    
    # Use raw file descriptors for direct binary I/O
    # Instead of using .buffer which might not be available
    raw_stdin = os.fdopen(0, 'rb', 0)  # fd 0 = stdin
    raw_stdout = os.fdopen(1, 'wb', 0)  # fd 1 = stdout
    
    # Save these for use in the message loop
    # DO NOT override sys.stdin/stdout completely as that can break other code
    # We'll use these raw handles directly in the message reading/writing code
    original_stdin = sys.stdin
    original_stdout = sys.stdout
    
    sys.stderr.write("SETUP: Successfully configured binary I/O\n") 
    sys.stderr.flush()
except Exception as e:
    sys.stderr.write(f"CRITICAL SETUP ERROR: Failed to configure binary I/O: {e}\n")
    sys.stderr.write(traceback.format_exc() + "\n")
    sys.stderr.flush()
    # Don't exit - let the server try to continue with normal I/O

# Only import if TYPE_CHECKING
try:
    from typing import TYPE_CHECKING
    if TYPE_CHECKING:
        from typing import Callable
except ImportError:
    pass

import fcntl
import select
import errno

class McpBaseServer:
    """Base class for MCP servers handling JSON-RPC over stdio."""

    def __init__(self, name: str, version: str, tools: List[Dict]):
        self.name = name
        self.version = version
        self.tools = tools
        self.tool_handlers: Dict[str, Callable] = {}
        self.logger = self._setup_logging()
        self._shutdown_requested = False
        self._initialized = False
        
        # Log initialization
        self.logger.info(f"Initialized {self.name} server v{self.version}")

    def _setup_logging(self):
        # Configure logging to a file in /tmp for easier debugging
        log_file = f'/tmp/{self.name}.log'
        
        # Remove old log file if it exists for cleaner logs
        if os.path.exists(log_file):
            try:
                os.remove(log_file)
            except OSError as e:
                sys.stderr.write(f"Warning: Could not remove old log file {log_file}: {e}\n")
                sys.stderr.flush()
                
        logging.basicConfig(
            level=logging.INFO,
            format='%(asctime)s - %(name)s - %(levelname)s - %(message)s',
            handlers=[
                logging.FileHandler(log_file),
                # Optional: Add stderr logging for errors/critical logs
                # logging.StreamHandler(sys.stderr) 
            ]
        )
        logger = logging.getLogger(self.name)
        
        # Add a stderr handler for ERROR/CRITICAL logs
        stderr_handler = logging.StreamHandler(sys.stderr)
        stderr_handler.setLevel(logging.ERROR)
        formatter = logging.Formatter('%(asctime)s - %(name)s - %(levelname)s - %(message)s')
        stderr_handler.setFormatter(formatter)
        logger.addHandler(stderr_handler)
        
        logger.info(f"Logging initialized. Log file: {log_file}")
        return logger

    def register_tool(self, name: str, handler: Callable):
        """Registers a handler function for a specific tool."""
        self.logger.info(f"Registering tool: {name}")
        self.tool_handlers[name] = handler

    def _create_capabilities(self) -> Dict:
        """Creates the capabilities dictionary for the initialize response."""
        return {
            "serverInfo": {
                "name": self.name,
                "version": self.version
            },
            "capabilities": {
                "tools": self.tools,
                "resources": [] # Assuming no resources for now
            },
            "status": "initialized" # Add status field expected by the host
        }

    def _handle_request(self, request: Dict) -> Optional[Dict]:
        """Handles a received JSON-RPC request dictionary."""
        method = request.get("method")
        params = request.get("params", {})
        req_id = request.get("id") # Can be None for notifications

        self.logger.info(f"Handling request: method='{method}', id={req_id}")
        self.logger.debug(f"Request params: {params}")
        sys.stderr.write(f"DEBUG: Handling request: method='{method}', id={req_id}, params={params!r}\n")
        sys.stderr.flush()

        response = None
        if method == "initialize":
            self.logger.info("Received initialize request.")
            sys.stderr.write("DEBUG: Processing initialize request\n")
            sys.stderr.flush()
            
            self._initialized = True # Mark server as initialized
            if req_id is not None:
                capabilities = self._create_capabilities()
                response = {"jsonrpc": "2.0", "id": req_id, "result": capabilities}
                sys.stderr.write(f"DEBUG: Sending capabilities response for ID {req_id}: {capabilities!r}\n")
                sys.stderr.flush()
            else:
                # Initialize *must* be a request, not notification
                self.logger.error("Initialize request received without an ID.")
                sys.stderr.write("ERROR: Initialize request received without an ID\n")
                sys.stderr.flush()
                # Cannot send error response without ID.

        elif method == "shutdown":
            self.logger.info("Received shutdown request.")
            sys.stderr.write("DEBUG: Processing shutdown request\n")
            sys.stderr.flush()
            
            self._shutdown_requested = True
            if req_id is not None:
                 response = {"jsonrpc": "2.0", "id": req_id, "result": None}
            # If it's a notification, we just set the flag and exit later.

        elif method == "exit":
            # Exit notification, no response needed.
            self.logger.info("Received exit notification. Exiting loop.")
            sys.stderr.write("DEBUG: Processing exit notification\n")
            sys.stderr.flush()
            
            self._shutdown_requested = True 

        elif method == "tool/execute" or method == "mcp/tool/execute":  # Handle both method names for compatibility
            # Note the correct method name for MCP
            if req_id is None:
                self.logger.error("tool/execute received as notification, requires ID.")
                sys.stderr.write("ERROR: tool/execute received without ID\n")
                sys.stderr.flush()
                return None # Cannot respond

            # Extract tool name and arguments from the correct structure
            tool_name = params.get("tool_name") if isinstance(params, dict) else None
            arguments = params.get("arguments", {}) if isinstance(params, dict) else {}
            
            # Also try extract from 'name' field if 'tool_name' is not present
            if tool_name is None and isinstance(params, dict):
                tool_name = params.get("name")
                
            self.logger.info(f"Received tool/execute for tool: {tool_name}")
            self.logger.debug(f"Tool arguments: {arguments}")
            sys.stderr.write(f"DEBUG: Processing tool/execute for tool: {tool_name}, args: {arguments!r}\n")
            sys.stderr.flush()

            handler = self.tool_handlers.get(tool_name)
            if handler:
                try:
                    # Execute the handler
                    result = handler(arguments) 
                    self.logger.info(f"Tool '{tool_name}' executed successfully.")
                    self.logger.debug(f"Tool result: {result}")
                    sys.stderr.write(f"DEBUG: Tool '{tool_name}' executed successfully\n")
                    sys.stderr.flush()
                    
                    response = {"jsonrpc": "2.0", "id": req_id, "result": result}
                except Exception as e:
                    self.logger.error(f"Error executing tool '{tool_name}': {e}\n{traceback.format_exc()}")
                    sys.stderr.write(f"ERROR: Tool '{tool_name}' execution failed: {e}\n")
                    sys.stderr.flush()
                    
                    response = {
                        "jsonrpc": "2.0",
                        "id": req_id,
                        "error": {"code": -32000, "message": f"Tool execution error: {e}"}
                    }
            else:
                self.logger.error(f"Tool not found: {tool_name}")
                sys.stderr.write(f"ERROR: Tool not found: {tool_name}\n")
                sys.stderr.flush()
                
                response = {
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "error": {"code": -32601, "message": f"Tool not found: {tool_name}"}
                }

        else:
            # Handle other standard LSP notifications like $/progress, $/logTrace if needed
            if method and method.startswith('$/'):
                self.logger.info(f"Received LSP notification '{method}', ignoring.")
                sys.stderr.write(f"DEBUG: Ignoring LSP notification '{method}'\n")
                sys.stderr.flush()
                # No response needed for notifications
            elif req_id is not None:
                 # Unknown method that requires a response
                 self.logger.error(f"Method not found: {method}")
                 sys.stderr.write(f"ERROR: Method not found: {method}\n")
                 sys.stderr.flush()
                 
                 response = {
                    "jsonrpc": "2.0", 
                    "id": req_id, 
                    "error": {"code": -32601, "message": f"Method not found: {method}"}
                 }
            else:
                # Unknown notification
                self.logger.warning(f"Received unknown notification: method='{method}'")
                sys.stderr.write(f"WARN: Unknown notification: method='{method}'\n")
                sys.stderr.flush()

        return response

    def _send_response(self, response: Dict):
        """Send a JSON-RPC response with proper Content-Length headers."""
        try:
            response_json = json.dumps(response)
            response_bytes = response_json.encode('utf-8')
            content_length = len(response_bytes)
            
            # Create header with strict \r\n line endings
            header = f"Content-Length: {content_length}\r\n\r\n".encode('utf-8')
            
            self.logger.info(f"Sending response for request ID: {response.get('id')} (Length: {content_length})")
            self.logger.debug(f"Response content: {response_json}")
            
            # Force binary mode for direct stdout writing
            try:
                # Write directly to binary stdout in one operation if possible
                raw_stdout.write(header + response_bytes)
                raw_stdout.flush()
                self.logger.debug("Response sent and flushed successfully (single write)")
            except (AttributeError, IOError):
                # Fallback: separate writes with explicit flush after each
                self.logger.warning("Using fallback send method (separate writes)")
                raw_stdout.write(header)
                raw_stdout.flush() # Flush after header
                
                raw_stdout.write(response_bytes)
                raw_stdout.flush() # Flush after content
                
                self.logger.debug("Response sent and flushed successfully (separate writes)")
                
        except Exception as e:
            self.logger.error(f"Failed to send response: {e}", exc_info=True)
            # Try to log to stderr as a last resort
            try:
                sys.stderr.write(f"CRITICAL ERROR: Failed to send response: {e}\n")
                sys.stderr.flush()
            except:
                pass

    def run(self):
        """Starts the server's main execution loop."""
        self.logger.info(f"Starting {self.name} MCP server v{self.version}. Reading from stdin, writing to stdout.")
        self.logger.info(f"Using stdin (fd: {sys.stdin.fileno()}), stdout (fd: {sys.stdout.fileno()}), stderr (fd: {sys.stderr.fileno()})")
        
        # Try to ensure stdout is usable for binary writing
        try:
            if hasattr(sys.stdout, 'buffer'):
                 self.logger.info("Using sys.stdout.buffer for binary output.")
            else:
                 self.logger.warning("sys.stdout.buffer not available, attempting direct binary write.")
                 sys.stdout = os.fdopen(sys.stdout.fileno(), 'wb', buffering=0)
        except Exception as e:
            self.logger.error(f"Could not set stdout to binary mode: {e}", exc_info=True)
            sys.stderr.write(f"CRITICAL: Failed to configure stdout for binary writing: {e}\n")
            sys.stderr.flush()
            sys.exit(1)

        # Handle signals for graceful shutdown
        try:
            signal.signal(signal.SIGINT, self._signal_handler)
            signal.signal(signal.SIGTERM, self._signal_handler)
            self.logger.info("Registered signal handlers for SIGINT and SIGTERM.")
        except ValueError:
            self.logger.warning("Could not set signal handlers (possibly not in main thread).")
        except Exception as e:
            self.logger.error(f"Failed to set signal handlers: {e}", exc_info=True)

        try:
            # Use the new byte-by-byte reading for Content-Length headers
            self._run_message_loop()
        except KeyboardInterrupt:
            self.logger.info("Received keyboard interrupt, shutting down")
        except Exception as e:
            self.logger.critical(f"Unhandled exception in run loop: {e}", exc_info=True)
        finally:
            self.logger.info(f"Shutting down {self.name}.")

    def _run_message_loop(self):
        """The main message processing loop using a more robust header parsing approach."""
        self.logger.info("Starting message loop, waiting for first message...")
        
        # Add a direct debug message to stderr for visibility
        sys.stderr.write("DEBUG: Entering message loop, waiting for first message\n")
        sys.stderr.flush()
        
        while not self._shutdown_requested:
            try:
                # Use our more robust header and content reading method
                header_text, content_text = self._read_header_and_content()
                
                if header_text is None or content_text is None:
                    self.logger.error("Failed to read a valid message")
                    sys.stderr.write("DEBUG: Failed to read a valid message, skipping\n")
                    sys.stderr.flush()
                    # If we get empty input, it could mean EOF
                    if not header_text and not content_text:
                        self._shutdown_requested = True
                    continue
                
                # Process the content as JSON
                try:
                    request = json.loads(content_text)
                    if not isinstance(request, dict):
                        self.logger.error(f"Request is not a JSON object: {type(request)}")
                        sys.stderr.write(f"DEBUG: Request is not a JSON object: {type(request)}\n")
                        sys.stderr.flush()
                        continue
                    
                    # Log the request (especially important for initialize)
                    method = request.get("method")
                    req_id = request.get("id")
                    self.logger.info(f"Processing {method} request (ID: {req_id})")
                    sys.stderr.write(f"DEBUG: Processing request - method: {method}, id: {req_id}\n")
                    sys.stderr.flush()
                        
                    # Handle the request with our business logic
                    response = self._handle_request(request)
                    
                    # Send response if there is one (notifications don't get responses)
                    if response:
                        self.logger.info(f"Sending response for request ID: {response.get('id')}")
                        sys.stderr.write(f"DEBUG: Sending response for ID: {response.get('id')}\n")
                        sys.stderr.flush()
                        self._send_response(response)
                        self.logger.debug(f"Response sent successfully for ID: {response.get('id')}")
                        sys.stderr.write(f"DEBUG: Response sent successfully for ID: {response.get('id')}\n")
                        sys.stderr.flush()
                        
                except json.JSONDecodeError as e:
                    self.logger.error(f"Invalid JSON in request: {e}", exc_info=True)
                    sys.stderr.write(f"DEBUG: JSON decode error: {e}\n")
                    sys.stderr.flush()
                    
                except Exception as e:
                    self.logger.error(f"Error processing message: {e}", exc_info=True)
                    sys.stderr.write(f"DEBUG: Error processing message: {e}\n")
                    sys.stderr.flush()
                    
                    # Try to extract message ID for error response
                    request_id = None
                    try:
                        # Look for an ID in the broken JSON 
                        id_match = re.search(r'"id"\s*:\s*([0-9]+|"[^"]*")', content_text)
                        if id_match:
                            id_str = id_match.group(1)
                            request_id = json.loads(id_str) # Works for both numbers and strings
                    except:
                        pass
                    
                    # Send error response if we could extract an ID
                    if request_id is not None:
                        error_response = {
                            "jsonrpc": "2.0",
                            "id": request_id,
                            "error": {
                                "code": -32603,
                                "message": f"Internal error: {str(e)}"
                            }
                        }
                        self._send_response(error_response)
                    
            except KeyboardInterrupt:
                self.logger.info("Keyboard interrupt received, shutting down")
                sys.stderr.write("DEBUG: Keyboard interrupt received, shutting down\n")
                sys.stderr.flush()
                self._shutdown_requested = True
                
            except Exception as e:
                self.logger.critical(f"Unhandled error in message loop: {e}", exc_info=True)
                sys.stderr.write(f"DEBUG: Unhandled error in message loop: {e}\n")
                sys.stderr.flush()

    def _signal_handler(self, signum, frame):
        """Handles SIGINT and SIGTERM."""
        self.logger.info(f"Received signal {signum}, initiating shutdown...")
        self._shutdown_requested = True

    def _read_header_and_content(self):
        """Read and parse a message from stdin using simpler, more robust method."""
        sys.stderr.write("DEBUG: Starting to read message with simplified method\n")
        sys.stderr.flush()
        
        # Read headers until we find the double newline
        header_data = b''
        
        # For debugging, print each byte received
        byte_count = 0
        byte_output = ""
        
        # Read in larger chunks for better performance
        while True:
            chunk = raw_stdin.read(1)  # Still read byte by byte for header detection
            if not chunk:  # EOF
                sys.stderr.write("DEBUG: EOF while reading header\n")
                sys.stderr.flush()
                return None, None
            
            # Debug output for first few bytes
            byte_count += 1
            if byte_count <= 50:  # Show first 50 bytes
                byte_hex = ' '.join(f'{b:02x}' for b in chunk)
                byte_output += f"Byte {byte_count}: hex={byte_hex}, ascii={repr(chunk)}\n"
                if byte_count % 10 == 0 or byte_count == 1:  # Print in groups
                    sys.stderr.write(f"DEBUG: Received bytes:\n{byte_output}")
                    sys.stderr.flush()
                    byte_output = ""
                
            header_data += chunk
            
            # Check for double-newline sequences in various formats
            if b'\r\n\r\n' in header_data:
                sys.stderr.write("DEBUG: Found CRLF+CRLF header terminator\n")
                sys.stderr.flush()
                break
            if b'\n\n' in header_data:
                sys.stderr.write("DEBUG: Found LF+LF header terminator\n")
                sys.stderr.flush()
                break
                
            # Check for the literal backslashed string the host is sending
            # This is what's happening in our case - we're seeing \r\n\r\n as the string
            if b'\\r\\n\\r\\n' in header_data:
                sys.stderr.write("DEBUG: Found literal \\r\\n\\r\\n header terminator!\n")
                sys.stderr.flush()
                
                # Find the position right after the terminator
                pos = header_data.find(b'\\r\\n\\r\\n') + 8  # 8 is length of \\r\\n\\r\\n
                
                # Reconstruct the header with the raw bytes that follow the terminator
                content_start = header_data[pos:]
                
                # Parse Content-Length and directly read the rest from the stream
                match = re.search(rb'Content-Length: (\d+)', header_data)
                if not match:
                    sys.stderr.write("DEBUG: No Content-Length found in header\n")
                    sys.stderr.flush()
                    return None, None
                    
                content_length = int(match.group(1))
                sys.stderr.write(f"DEBUG: Content-Length: {content_length}\n")
                sys.stderr.flush()
                
                # We already have part of the content, read the rest
                bytes_needed = content_length - len(content_start)
                sys.stderr.write(f"DEBUG: Already have {len(content_start)} bytes, need {bytes_needed} more\n")
                sys.stderr.flush()
                
                # Read the rest of the content
                remaining_content = b''
                if bytes_needed > 0:
                    remaining_content = raw_stdin.read(bytes_needed)
                
                # Combine what we have with what we read
                content_data = content_start + remaining_content
                
                # Skip the rest of the function and handle specially
                try:
                    header_text = header_data[:pos].decode('utf-8')
                    content_text = content_data.decode('utf-8')
                    sys.stderr.write(f"DEBUG: Successfully parsed message with special handling. Content: {content_text!r}\n")
                    sys.stderr.flush()
                    return header_text, content_text
                except UnicodeDecodeError as e:
                    sys.stderr.write(f"DEBUG: Unicode decode error in special handling: {e}\n")
                    sys.stderr.flush()
                    return None, None
            
            # Prevent infinite loop on malformed input
            if len(header_data) > 4096:
                sys.stderr.write("DEBUG: Header too large, giving up\n")
                sys.stderr.flush()
                return None, None
        
        # Print any remaining debug bytes
        if byte_output:
            sys.stderr.write(f"DEBUG: Received bytes (final):\n{byte_output}")
            sys.stderr.flush()
                
        # Now we have the complete header
        sys.stderr.write(f"DEBUG: Got complete header ({len(header_data)} bytes): {header_data!r}\n")
        sys.stderr.flush()
        
        # Parse Content-Length
        # Fixed regex pattern using rb prefix for raw binary pattern
        match = re.search(rb'Content-Length: (\d+)', header_data)
        if not match:
            sys.stderr.write("DEBUG: No Content-Length found in header\n")
            sys.stderr.flush()
            return None, None
            
        content_length = int(match.group(1))
        sys.stderr.write(f"DEBUG: Content-Length: {content_length}\n")
        sys.stderr.flush()
        
        # Read exactly content_length bytes for the message body
        sys.stderr.write(f"DEBUG: Reading {content_length} bytes of content...\n")
        sys.stderr.flush()
        content_data = raw_stdin.read(content_length)
        
        sys.stderr.write(f"DEBUG: Read {len(content_data)} bytes of content\n")
        sys.stderr.flush()
        
        if len(content_data) != content_length:
            sys.stderr.write(f"DEBUG: Incomplete content read: got {len(content_data)}, expected {content_length}\n")
            sys.stderr.flush()
            return None, None
            
        # Decode header and content
        try:
            header_text = header_data.decode('utf-8')
            content_text = content_data.decode('utf-8')
            sys.stderr.write(f"DEBUG: Successfully parsed message. Content: {content_text!r}\n")
            sys.stderr.flush()
            return header_text, content_text
        except UnicodeDecodeError as e:
            sys.stderr.write(f"DEBUG: Unicode decode error: {e}\n")
            sys.stderr.flush()
            return None, None

# Demo usage if run directly
if __name__ == "__main__":
    # Example for testing
    class EchoServer(McpBaseServer):
        def __init__(self):
            tools = [
                {"name": "echo", "description": "Echoes back the input"},
                {"name": "fail", "description": "A tool designed to fail"}
            ]
            super().__init__("echo-server", "1.0.0", tools)
            self.register_tool("echo", self.handle_echo)
            self.register_tool("fail", self.handle_fail)
            
        def handle_echo(self, args):
            text = args.get("text", "No text provided")
            return {"message": f"Echo: {text}"}
            
        def handle_fail(self, args):
            raise ValueError("This tool failed intentionally!")
            
    server = EchoServer()
    server.run()

