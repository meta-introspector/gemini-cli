#!/usr/bin/env python3
"""
E5 Embedding Model MCP Server

This server provides embeddings using the E5 models from sentence-transformers.
It communicates via JSON-RPC over stdin/stdout for integration with Gemini CLI.
"""

import json
import logging
import sys
import os
import re
import traceback
from enum import Enum
from typing import Dict, List, Optional, Union, Any

import numpy as np
import torch
from pydantic import BaseModel, Field
from sentence_transformers import SentenceTransformer
from jsonrpcserver import method, Result, Success, Error, dispatch

# Configure logging
logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(levelname)s] %(message)s",
    handlers=[logging.FileHandler("mcp_embedding.log"), logging.StreamHandler(sys.stderr)]
)
logger = logging.getLogger(__name__)

# Set excepthook to log unhandled exceptions
def log_uncaught_exceptions(exc_type, exc_value, exc_traceback):
    logger.critical("Unhandled exception", exc_info=(exc_type, exc_value, exc_traceback))
    sys.__excepthook__(exc_type, exc_value, exc_traceback)

sys.excepthook = log_uncaught_exceptions

# Disable sentence-transformers progress bars in non-interactive mode
os.environ["TOKENIZERS_PARALLELISM"] = "true"


class ModelVariant(str, Enum):
    """Available E5 model variants."""
    SMALL = "small"
    BASE = "base"
    LARGE = "large"


# Global variables
model = None
model_variant = None
model_dimension = {
    "small": 384,
    "base": 768,
    "large": 1024
}


class InitializeRequest(BaseModel):
    """Request to initialize the server."""
    variant: ModelVariant = Field(default=ModelVariant.BASE, description="Model variant to use")
    clientInfo: Dict[str, str] = Field(default_factory=dict, description="Client information")


class EmbedRequest(BaseModel):
    """Request to generate embeddings."""
    text: str = Field(..., description="Text to embed")
    is_query: bool = Field(default=False, description="Whether this is a query (vs passage)")
    variant: Optional[ModelVariant] = Field(
        default=None, description="Optional override for model variant"
    )


@method
def initialize(params: Dict[str, Any]) -> Result:
    """Initialize the embedding model, responding immediately while loading the model in the background."""
    global model, model_variant
    
    # Parse request using Pydantic
    try:
        request = InitializeRequest(**params)
    except Exception as e:
        logger.error(f"Invalid initialization parameters: {e}")
        return Error(code=400, message=f"Invalid parameters: {str(e)}")
    
    variant = request.variant
    model_variant = variant
    model_name = f"intfloat/multilingual-e5-{variant}"
    
    # Respond immediately with basic info
    logger.info(f"Initialize request received for E5 model variant: {variant}")
    logger.info(f"Responding immediately, will load model '{model_name}' on first use")
    
    # Return server info and capabilities in MCP format
    return Success({
        "serverInfo": {
            "name": "embedding-mcp",
            "version": "1.0.0"
        },
        "capabilities": {
            "tools": [
                {
                    "name": "embed",
                    "description": "Generate embeddings from text using E5 models",
                    "schema": {
                        "type": "object",
                        "properties": {
                            "text": {
                                "type": "string",
                                "description": "Text to embed"
                            },
                            "is_query": {
                                "type": "boolean",
                                "description": "Whether this is a query (vs passage)",
                                "default": False
                            },
                            "variant": {
                                "type": "string",
                                "enum": ["small", "base", "large"],
                                "description": "Model variant to use"
                            }
                        },
                        "required": ["text"]
                    }
                }
            ],
            "resources": []
        },
        "status": "initialized"
    })


@method
def capabilities() -> Result:
    """Return the server's capabilities."""
    return Success({
        "name": "embedding",
        "capabilities": [
            {
                "toolName": "embedding/embed",
                "description": "Generate embeddings from text using E5 models",
                "parameters": {
                    "text": "Text to embed",
                    "is_query": "Whether this is a query (vs passage)",
                    "variant": "Model variant (small, base, large)"
                }
            }
        ]
    })


@method
def embed(params: Dict[str, Any]) -> Result:
    """Generate an embedding for the given text, loading the model if needed."""
    global model, model_variant
    
    # Check if model is initialized
    if model is None:
        # Try to initialize with default or requested settings
        logger.info("Model not initialized, loading now...")
        try:
            variant = model_variant or "base"  # Use stored variant or default to base
            model_name = f"intfloat/multilingual-e5-{variant}"
            
            # Load the model - this may take some time
            logger.info(f"Loading E5 model variant: {variant}")
            model = SentenceTransformer(model_name)
            
            # Basic info about the loaded model
            device = "GPU" if next(model.parameters()).is_cuda else "CPU"
            logger.info(f"Model loaded successfully on {device}")
        except Exception as e:
            logger.error(f"Failed to initialize model: {e}")
            return Error(code=500, message=f"Model initialization failed: {str(e)}")
    
    # Parse request
    try:
        request = EmbedRequest(**params)
    except Exception as e:
        logger.error(f"Invalid embed parameters: {e}")
        return Error(code=400, message=f"Invalid parameters: {str(e)}")
    
    # Handle variant override that requires model reload
    if request.variant and request.variant != model_variant:
        logger.info(f"Switching model variant from {model_variant} to {request.variant}")
        try:
            model_variant = request.variant
            model_name = f"intfloat/multilingual-e5-{model_variant}"
            
            # Reload the model with new variant
            model = SentenceTransformer(model_name)
            device = "GPU" if next(model.parameters()).is_cuda else "CPU"
            logger.info(f"Model switched successfully to {model_variant} on {device}")
        except Exception as e:
            logger.error(f"Failed to switch model: {e}")
            return Error(code=500, message=f"Model switching failed: {str(e)}")
    
    # Prefix the text according to E5 requirements
    if request.is_query:
        prefixed_text = f"query: {request.text}"
    else:
        prefixed_text = f"passage: {request.text}"
    
    try:
        # Generate the embedding
        with torch.no_grad():
            embedding = model.encode(prefixed_text, normalize_embeddings=True)
        
        # Convert to native Python list (for JSON serialization)
        embedding_list = embedding.tolist()
        
        return Success({
            "embedding": embedding_list,
            "dimension": len(embedding_list)
        })
    except Exception as e:
        logger.error(f"Embedding generation failed: {e}")
        return Error(code=500, message=f"Embedding generation failed: {str(e)}")


def handle_jsonrpc():
    """Handle JSON-RPC requests from stdin with Content-Length framing."""
    logger.info("Starting JSON-RPC message handler")
    logger.info(f"Python version: {sys.version}")
    logger.info(f"Using stderr for logging: {sys.stderr.fileno()}")
    logger.info(f"Using stdout for responses: {sys.stdout.fileno()}")
    logger.info(f"Using stdin for requests: {sys.stdin.fileno()}")
    
    # Try to put stdout in binary mode for reliable binary output
    try:
        sys.stdout = os.fdopen(sys.stdout.fileno(), 'wb')
        logger.info("Set stdout to binary mode")
    except Exception as e:
        logger.warning(f"Couldn't set stdout to binary mode: {e}")
    
    try:
        # First, handle initialize request immediately - don't wait for other code
        # Send a direct initialization response to ensure quick startup
        initialization_response = {
            "jsonrpc": "2.0",
            "id": 4,  # Typical ID for embedding server
            "result": {
                "serverInfo": {
                    "name": "embedding-mcp",
                    "version": "1.0.0"
                },
                "capabilities": {
                    "tools": [
                        {
                            "name": "embed",
                            "description": "Generate embeddings from text using E5 models"
                        }
                    ],
                    "resources": []
                },
                "status": "initialized"
            }
        }
        
        # Direct early response to ensure initialization works
        response_json = json.dumps(initialization_response)
        response_bytes = response_json.encode('utf-8')
        content_length = len(response_bytes)
        
        logger.info(f"Sending immediate initialization response with length: {content_length}")
        
        # Write early initialization response
        sys.stdout.write(f"Content-Length: {content_length}\r\n\r\n".encode('utf-8'))
        sys.stdout.write(response_bytes)
        sys.stdout.flush()
        
        logger.info("Early initialization response sent successfully")
    except Exception as e:
        logger.critical(f"Failed to send early initialization response: {e}")
        logger.critical(traceback.format_exc())
    
    # Continue with normal processing loop
    while True:
        try:
            # Read headers until empty line
            content_length = None
            headers_done = False
            
            while not headers_done:
                line = sys.stdin.readline()
                if not line:  # EOF
                    logger.info("Stdin closed (EOF). Exiting.")
                    return
                
                line = line.strip()
                if not line:  # Empty line marks end of headers
                    headers_done = True
                    continue
                
                # Parse Content-Length header
                if line.startswith("Content-Length:"):
                    try:
                        # Extract only the numeric part from the header
                        header_value = line.split(":", 1)[1].strip()
                        # Get only digits from the header value
                        numeric_part = ''.join(c for c in header_value if c.isdigit())
                        content_length = int(numeric_part)
                        logger.info(f"Received header: Content-Length = {content_length}")
                    except ValueError as e:
                        logger.error(f"Invalid Content-Length format: {line} - {e}")
            
            # Check if we have Content-Length header
            if content_length is None:
                logger.error("No Content-Length header found, skipping message")
                continue
            
            # Read exactly content_length bytes
            content = ''
            remaining = content_length
            while remaining > 0:
                chunk = sys.stdin.read(min(1024, remaining))
                if not chunk:  # EOF
                    logger.error("Unexpected EOF while reading content")
                    return
                content += chunk
                remaining -= len(chunk)
            
            logger.info(f"Received request content: {content}")
            
            # Process the request
            request = json.loads(content)
            
            # Special handling for initialization
            if request.get("method") == "initialize":
                logger.info(f"Received initialize request (id: {request.get('id')})")
                
                # Direct response to initialization request without using jsonrpcserver
                response = {
                    "jsonrpc": "2.0",
                    "id": request.get("id"),
                    "result": {
                        "serverInfo": {
                            "name": "embedding-mcp",
                            "version": "1.0.0"
                        },
                        "capabilities": {
                            "tools": [
                                {
                                    "name": "embed",
                                    "description": "Generate embeddings from text using E5 models"
                                }
                            ],
                            "resources": []
                        },
                        "status": "initialized"
                    }
                }
                
                # Convert response to JSON string
                response_json = json.dumps(response)
                
                # Send response with proper Content-Length framing
                response_bytes = response_json.encode('utf-8')
                content_length = len(response_bytes)
                
                logger.info(f"Sending initialization response: {response_json}")
                logger.info(f"Response length: {content_length}")
                
                # Write headers with strict \r\n format
                header = f"Content-Length: {content_length}\r\n\r\n"
                sys.stdout.write(header.encode('utf-8'))
                sys.stdout.flush()
                
                # Write content
                sys.stdout.write(response_bytes)
                sys.stdout.flush()
                
                logger.info("Initialization response sent successfully")
                continue
                
            elif request.get("method") == "tool/execute":
                # Handle tool execution requests
                params = request.get("params", {})
                tool_name = params.get("tool_name")
                arguments = params.get("arguments", {})
                
                logger.info(f"Received tool/execute request for tool: {tool_name}")
                
                if tool_name == "embed":
                    # Call the embed method with arguments
                    logger.info(f"Executing embed tool with arguments: {arguments}")
                    try:
                        # Directly execute embed functionality
                        result = embed(arguments)
                        response = {
                            "jsonrpc": "2.0",
                            "id": request.get("id"),
                            "result": result.data
                        }
                    except Exception as e:
                        logger.error(f"Embed tool execution failed: {e}")
                        logger.error(traceback.format_exc())
                        response = {
                            "jsonrpc": "2.0",
                            "id": request.get("id"),
                            "error": {
                                "code": -32000,
                                "message": f"Tool execution failed: {str(e)}"
                            }
                        }
                    
                    logger.info(f"Sending tool/execute response for ID: {request.get('id')}")
                    send_response(response)
                    continue
                else:
                    # Unknown tool
                    logger.error(f"Unknown tool: {tool_name}")
                    send_error_response(-32601, f"Tool not found: {tool_name}", request.get("id"))
                    continue
            elif request.get("method") == "shutdown":
                # Handle shutdown request
                logger.info("Received shutdown request")
                response = {
                    "jsonrpc": "2.0",
                    "id": request.get("id"),
                    "result": None  # Use null (None in Python) for result as per spec
                }
                
                # Log the response we're sending
                logger.info(f"Sending shutdown response for ID: {request.get('id')}")
                
                # Convert response to JSON string
                response_json = json.dumps(response)
                response_bytes = response_json.encode('utf-8')
                content_length = len(response_bytes)
                
                # Log the content being sent
                logger.info(f"Shutdown response content: {response_json}")
                logger.info(f"Shutdown response length: {content_length}")
                
                try:
                    # Write the header
                    header = f"Content-Length: {content_length}\r\n\r\n".encode('utf-8')
                    sys.stdout.write(header)
                    sys.stdout.flush()
                    logger.info("Shutdown response header sent")
                    
                    # Write content
                    sys.stdout.write(response_bytes)
                    sys.stdout.flush()
                    logger.info("Shutdown response sent successfully. Exiting.")
                    
                    # Exit the loop/function after sending the response
                    return
                except Exception as e:
                    logger.error(f"Failed to send shutdown response: {e}")
                    logger.error(traceback.format_exc())
                    return  # Still exit even if response fails
            elif request.get("method") == "exit":
                # Handle exit request
                logger.info("Received exit request, terminating")
                return
            
            # Default dispatch for other methods
            logger.info(f"Using default dispatch for method: {request.get('method')}")
            response = dispatch(request)
            send_response(response)
            
        except json.JSONDecodeError as e:
            logger.error(f"Invalid JSON: {e}")
            send_error_response(-32700, "Parse error", request_id=None)
        except Exception as e:
            logger.error(f"Error processing request: {e}")
            logger.error(traceback.format_exc())
            send_error_response(-32603, f"Internal error: {str(e)}", request_id=None)


def send_response(response):
    """Send a JSON-RPC response with proper Content-Length framing."""
    try:
        # First serialize to JSON
        try:
            response_json = json.dumps(response)
        except Exception as e:
            logger.error(f"Failed to serialize response to JSON: {e}")
            logger.error(traceback.format_exc())
            # Try to send a simpler error response
            if response.get("method") == "shutdown":
                logger.info("Attempting to send simplified shutdown response")
                response_json = json.dumps({"jsonrpc": "2.0", "id": response.get("id"), "result": None})
            else:
                raise
        
        # Convert to bytes
        response_bytes = response_json.encode('utf-8')
        content_length = len(response_bytes)
        
        logger.info(f"Sending response for request ID: {response.get('id')}")
        logger.debug(f"Response content: {response_json}")
        logger.info(f"Response length: {content_length} bytes")
        
        # Write headers with strict \r\n format - ensure no extra content in the header
        header = f"Content-Length: {content_length}\r\n\r\n"
        header_bytes = header.encode('utf-8')
        
        # Write the header and flush
        try:
            sys.stdout.write(header_bytes)
            sys.stdout.flush()
            logger.debug(f"Header written: {header.strip()}")
        except Exception as e:
            logger.error(f"Failed to write response header: {e}")
            logger.error(traceback.format_exc())
            raise
        
        # Write content and flush
        try:
            sys.stdout.write(response_bytes)
            sys.stdout.flush()
            logger.debug("Content written and flushed")
        except Exception as e:
            logger.error(f"Failed to write response content: {e}")
            logger.error(traceback.format_exc())
            raise
        
        logger.info(f"Response for ID {response.get('id')} sent successfully")
        
        # Special handling for shutdown
        if response.get("method") == "shutdown" or (
            "result" in response and response.get("id") is not None and 
            isinstance(response.get("id"), (int, str)) and 
            "shutdown" in str(response)
        ):
            logger.info("Shutdown response sent, will exit soon")
    except Exception as e:
        logger.error(f"Failed to send response: {e}")
        logger.error(traceback.format_exc())
        # Don't raise the exception, as this would prevent the server from continuing operation


def send_error_response(code, message, request_id=None):
    """Send error response with proper JSON-RPC format."""
    error_response = {
        "jsonrpc": "2.0",
        "id": request_id,
        "error": {
            "code": code,
            "message": message
        }
    }
    logger.info(f"Sending error response: code={code}, message={message}")
    send_response(error_response)


if __name__ == "__main__":
    try:
        handle_jsonrpc()
    except KeyboardInterrupt:
        logger.info("Received keyboard interrupt, exiting gracefully.")
    except Exception as e:
        logger.critical(f"Unhandled exception in main: {e}")
        logger.critical(traceback.format_exc())
        sys.exit(1)
