use tokio::{io::{ AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter}, sync::mpsc, task::JoinHandle};

use crate::{McpError, McpMessage};

pub struct MessageFrame(pub String);

impl MessageFrame {
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

pub struct StdioTransport {
    pub sender: mpsc::Sender<MessageFrame>,
    pub receiver: mpsc::Receiver<MessageFrame>,
    read_task: Option<JoinHandle<Result<(),McpError>>>,
    write_task: Option<JoinHandle<Result<(),McpError>>>,
}

impl StdioTransport {
    pub fn new() -> Self {
        let (tx_in, rx_in) = mpsc::channel(100);
        let (tx_out, rx_out) = mpsc::channel(100);
        let read_handle = tokio::spawn(Self::read_loop(tx_in));
        let write_handle = tokio::spawn(Self::write_loop(rx_out));
        Self {
            sender: tx_out,
            receiver: rx_in,
            read_task: Some(read_handle),
            write_task: Some(write_handle),
        }
    }
/// Asynchronously reads lines from stdin and sends them to the receiver channel.    async fn read_loop(sender: mpsc::Sender<MessageFrame>) -> Result<(), McpError> {
    async fn read_loop(sender: mpsc::Sender<MessageFrame>) -> Result<(), McpError> {
        let stdin = tokio::io::stdin();
        let mut reader = BufReader::new(stdin).lines();
        while let Some(line) = reader.next_line().await? {
            if !line.trim().is_empty() {
                sender.send(MessageFrame(line)).await
                .map_err(|e| McpError::Io(std::io::Error::new(std::io::ErrorKind::BrokenPipe,format!("Failed to send incoming message to channel: {}",e))))?;
            }
        }
        Ok(())
    }
/// Asynchronously reads message frames from the sender channel and writes them to stdout.
    async fn write_loop(mut receiver: mpsc::Receiver<MessageFrame>) -> Result<(), McpError> {
       let stdout = tokio::io::stdout();
       let mut writer = BufWriter::new(stdout);
       while let Some(frame) = receiver.recv().await {
              writer.write_all(frame.as_bytes()).await?;
              writer.write_all(b"\n").await?;
              writer.flush().await?;
       }
       Ok(())
    }

    // send message to the transport
    pub async fn send(&self, message:McpMessage) -> Result<(), McpError> {
        let json_string = message.to_json()?;
        self.sender.send(MessageFrame(json_string)).await
            .map_err(|e| McpError::Io(std::io::Error::new(std::io::ErrorKind::BrokenPipe, format!("Failed to send outgoing message to transport: {}", e))))
    }

    /// Receives an MCP message from the transport.
    pub async fn recv(&mut self) -> Result<McpMessage, McpError> {
        match self.receiver.recv().await {
            Some(frame) => McpMessage::from_json(&frame.0),
            None => Err(McpError::Io(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                "Transport disconnected or read task stopped"
            ))),
        }
    }
    
}

impl Drop for StdioTransport {
    fn drop(&mut self) {
        // Abort the tasks when the transport is dropped to ensure graceful shutdown
        if let Some(handle) = self.read_task.take() {
            handle.abort();
        }
        if let Some(handle) = self.write_task.take() {
            handle.abort();
        }
    }
}