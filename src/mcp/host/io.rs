use log::{debug, error, info};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

// Helper loop for writing to stdin
pub async fn stdin_writer_loop<W: AsyncWrite + Unpin>(
    server_name: &str,
    writer: &mut tokio::io::BufWriter<W>,
    rx: &mut mpsc::Receiver<String>,
) {
    while let Some(json_message) = rx.recv().await {
        let message = format!(
            "Content-Length: {}

{}",
            json_message.len(),
            json_message
        );
        debug!("Sending to '{}': {}", server_name, message.trim_end());
        if let Err(e) = writer.write_all(message.as_bytes()).await {
            error!(
                "Error writing to stdin for '{}': {}. Stopping writer.",
                server_name, e
            );
            break; // Exit loop on write error
        }
        // Flush after each message to ensure timely delivery
        if let Err(e) = writer.flush().await {
            error!(
                "Error flushing stdin for '{}': {}. Stopping writer.",
                server_name, e
            );
            break;
        }
    }
    info!("Stdin channel closed for '{}'. Writer loop exiting.", server_name);
}

// Reads one complete JSON-RPC message based on Content-Length header
pub async fn read_json_rpc_message<R: AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
    buffer: &mut Vec<u8>,
) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    let mut content_length: Option<usize> = None;
    
    buffer.clear(); // Start with a clean buffer for headers

    // Read headers line by line
    loop {
        let bytes_read = reader.read_until(b'\n', buffer).await?;
        if bytes_read == 0 {
            // EOF before finding headers or content
            return if buffer.is_empty() {
                Ok(None) // Clean EOF
            } else {
                Err("Connection closed unexpectedly during headers".into())
            };
        }

        // Get just the current line from what we read
        let line_cow = String::from_utf8_lossy(&buffer[buffer.len() - bytes_read..]);
        let line = line_cow.trim_end();

        // Empty line means end of headers
        if line.is_empty() {
            buffer.clear(); // We're done with headers now
            break;
        }

        if line.starts_with("Content-Length:") {
            if let Some(len_str) = line.split(':').nth(1) {
                if let Ok(len) = len_str.trim().parse::<usize>() {
                    content_length = Some(len);
                } else {
                    return Err(format!("Invalid Content-Length value: {}", len_str).into());
                }
            }
        }
        // else if line.starts_with("Content-Type:") { /* Handle if needed */ }

        // Optimization: If buffer grows too large reading headers, something is wrong
        if buffer.len() > 4096 {
            return Err("Header section too large".into());
        }

        // Clear the buffer for the next line
        buffer.clear();
    }

    let length = content_length.ok_or("Missing Content-Length header")?;

    // Read the exact content length
    buffer.resize(length, 0); // Allocate space
    reader.read_exact(buffer).await?;

    String::from_utf8(buffer.to_vec()) // Convert buffer to String
        .map(Some)
        .map_err(|e| e.into())
} 