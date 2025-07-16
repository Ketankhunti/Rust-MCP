use bytes::{Buf, BufMut, BytesMut};
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::TcpStream};

use crate::{McpError, McpMessage};

const LENGTH_PREFIX_BYTES: usize = 4; // Use 4 bytes for length prefix

pub struct TcpTransport {
    stream: TcpStream,
    read_buffer: BytesMut
}

impl TcpTransport {
    pub  fn new(stream: TcpStream) -> Self {
        TcpTransport {
            stream,
            read_buffer: BytesMut::with_capacity(4096), // initial capacity of 4096 bytes
        }
    }

    pub async fn send(&mut self, message: McpMessage) -> Result<(), McpError> {
        let json_string = message.to_json()?;
        let payload_bytes = json_string.as_bytes();
        let payload_len = payload_bytes.len();

        if payload_len > u32::MAX as usize { // Check if payload fits in u32
            return Err(McpError::ProtocolError(format!("Message too large: {} bytes, exceeds u32 max.", payload_len)));
        }

        let mut write_buffer = BytesMut::with_capacity(LENGTH_PREFIX_BYTES + payload_len);
        write_buffer.put_u32(payload_len as u32); // Write length prefix
        write_buffer.put_slice(payload_bytes); // Write the actual message

        self.stream.write_all(&write_buffer).await?;
        self.stream.flush().await?;

        Ok(())

    }

    pub async fn recv(&mut self) -> Result<McpMessage, McpError> {
        loop { // Outer loop to keep trying to read messages
            // --- Phase 1: Read Length Prefix (4 bytes) ---
            // Ensure we have at least 4 bytes for the length prefix
            while self.read_buffer.len() < LENGTH_PREFIX_BYTES {
                self.read_buffer.reserve(512); // Reserve space to read into
                let bytes_read = self.stream.read_buf(&mut self.read_buffer).await?;
                if bytes_read == 0 {
                    // EOF: Connection closed by peer
                    return Err(McpError::Io(std::io::Error::new(
                        std::io::ErrorKind::ConnectionAborted,
                        "TCP stream closed by peer (EOF detected while reading length prefix)."
                    )));
                }
            }

            // At this point, self.read_buffer has at least 4 bytes.
            // Consume the length prefix bytes from the buffer and read the length.
            let mut length_prefix_bytes = self.read_buffer.split_to(LENGTH_PREFIX_BYTES); // CONSUMES 4 BYTES
            let payload_len = length_prefix_bytes.get_u32() as usize; // Read u32 and cast to usize

            let total_message_size_excluding_prefix = payload_len; // This is just the payload length

            // --- Phase 2: Read Message Payload ---
            // Ensure the entire payload is in the buffer
            while self.read_buffer.len() < total_message_size_excluding_prefix {
                self.read_buffer.reserve(total_message_size_excluding_prefix - self.read_buffer.len()); // Reserve exactly what's needed
                let bytes_read = self.stream.read_buf(&mut self.read_buffer).await?;
                if bytes_read == 0 {
                    // EOF: Connection closed by peer while reading payload
                    return Err(McpError::Io(std::io::Error::new(
                        std::io::ErrorKind::ConnectionAborted,
                        "TCP stream closed by peer (EOF detected while reading payload)."
                    )));
                }
            }

            // --- Phase 3: Extract and Process Payload ---
            let payload = self.read_buffer.split_to(total_message_size_excluding_prefix); // Extract the payload bytes

            let json_string = String::from_utf8(payload.to_vec())
                .map_err(|e| McpError::ProtocolError(format!("Invalid UTF-8 in payload: {}", e)))?;
           
            eprintln!("DEBUG: TcpTransport::recv - Raw JSON string received: '{}'", json_string);

            // Return the parsed message
            return McpMessage::from_json(&json_string);
        }
    }
}