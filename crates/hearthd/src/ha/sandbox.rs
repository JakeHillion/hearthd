//! Sandbox management for running Python integrations.
//!
//! Uses tokio::net::UnixStream::pair() to create a socketpair and passes
//! the file descriptor to the Python process via environment variable.
//! No filesystem paths are used.

use super::protocol::{Message, ProtocolError, Response};
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::{Child, Command};

/// Manages a sandboxed Python environment for running Home Assistant integrations.
pub struct Sandbox {
    /// Entry ID for this integration instance
    entry_id: String,

    /// Path to the Python executable
    python_path: PathBuf,

    /// Path to Home Assistant source (for integrations)
    ha_source_path: PathBuf,

    /// Tokio Unix stream for communication with Python
    stream: Option<BufReader<UnixStream>>,

    /// Child process handle
    child: Option<Child>,
}

impl Sandbox {
    /// Create a new sandbox instance
    pub fn new(entry_id: String, python_path: PathBuf, ha_source_path: PathBuf) -> Self {
        Self {
            entry_id,
            python_path,
            ha_source_path,
            stream: None,
            child: None,
        }
    }

    /// Start the Python process and connect to it
    pub async fn start(&mut self) -> Result<(), ProtocolError> {
        tracing::info!("[{}] Starting sandbox", self.entry_id);

        // Create socketpair for bidirectional communication
        let (rust_stream, python_stream) = UnixStream::pair().map_err(ProtocolError::Io)?;

        // Get file descriptor number for Python side
        let python_fd = python_stream.as_raw_fd();
        tracing::debug!(
            "[{}] Created socketpair, Python FD: {}",
            self.entry_id,
            python_fd
        );

        // Build command to spawn Python runner
        let mut cmd = Command::new(&self.python_path);
        cmd.arg("-u") // Unbuffered output
            .arg("python/runner.py")
            .env("HEARTHD_SOCKET_FD", python_fd.to_string())
            .env("HEARTHD_ENTRY_ID", &self.entry_id)
            .env("HEARTHD_HA_SOURCE", &self.ha_source_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Clear FD_CLOEXEC flag so socket survives exec
        unsafe {
            cmd.pre_exec(move || {
                let flags = libc::fcntl(python_fd, libc::F_GETFD);
                if flags == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                let result = libc::fcntl(python_fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC);
                if result == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }

        // Spawn the process
        let mut child = cmd.spawn().map_err(|e| {
            tracing::error!("[{}] Failed to spawn Python process: {}", self.entry_id, e);
            ProtocolError::Io(e)
        })?;

        tracing::info!(
            "[{}] Python process spawned (PID: {})",
            self.entry_id,
            child.id().unwrap_or(0)
        );

        // Capture stdout/stderr for logging
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let entry_id_clone = self.entry_id.clone();

        // Spawn tasks to log Python output
        if let Some(mut stdout) = stdout {
            let entry_id = entry_id_clone.clone();
            tokio::spawn(async move {
                use tokio::io::AsyncBufReadExt;
                let reader = tokio::io::BufReader::new(&mut stdout);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::info!("[{}] [stdout] {}", entry_id, line);
                }
            });
        }

        if let Some(mut stderr) = stderr {
            let entry_id = entry_id_clone;
            tokio::spawn(async move {
                use tokio::io::AsyncBufReadExt;
                let reader = tokio::io::BufReader::new(&mut stderr);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::warn!("[{}] [stderr] {}", entry_id, line);
                }
            });
        }

        // Store child process handle
        self.child = Some(child);

        // Drop Python side socket (child owns it now)
        drop(python_stream);

        // Wrap Rust side in buffered reader for line-based reading
        self.stream = Some(BufReader::new(rust_stream));

        tracing::debug!(
            "[{}] Sandbox started, waiting for Ready message",
            self.entry_id
        );

        Ok(())
    }

    /// Send a response to the Python process
    pub async fn send(&mut self, response: Response) -> Result<(), ProtocolError> {
        let stream = self.stream.as_mut().ok_or_else(|| {
            ProtocolError::Io(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "Sandbox not started",
            ))
        })?;

        // Serialize to JSON
        let json = serde_json::to_string(&response)?;

        tracing::trace!("[{}] Sending: {}", self.entry_id, json);

        // Write JSON + newline
        let inner = stream.get_mut();
        inner.write_all(json.as_bytes()).await?;
        inner.write_all(b"\n").await?;
        inner.flush().await?;

        Ok(())
    }

    /// Receive a message from the Python process
    pub async fn recv(&mut self) -> Result<Message, ProtocolError> {
        let stream = self.stream.as_mut().ok_or_else(|| {
            ProtocolError::Io(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "Sandbox not started",
            ))
        })?;

        // Read line (newline-delimited JSON)
        let mut line = String::new();
        stream.read_line(&mut line).await?;

        if line.is_empty() {
            return Err(ProtocolError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "Socket closed",
            )));
        }

        tracing::trace!("[{}] Received: {}", self.entry_id, line.trim());

        // Deserialize from JSON
        let message: Message = serde_json::from_str(line.trim())?;

        Ok(message)
    }

    /// Stop the Python process gracefully
    pub async fn stop(&mut self) -> Result<(), ProtocolError> {
        tracing::info!("[{}] Stopping sandbox", self.entry_id);

        // Send shutdown signal
        if self.stream.is_some() {
            let _ = self.send(Response::Shutdown).await;
        }

        // Wait for process to exit
        if let Some(mut child) = self.child.take() {
            match tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await {
                Ok(Ok(status)) => {
                    tracing::info!("[{}] Process exited with status: {}", self.entry_id, status);
                }
                Ok(Err(e)) => {
                    tracing::error!("[{}] Failed to wait for process: {}", self.entry_id, e);
                }
                Err(_) => {
                    tracing::warn!(
                        "[{}] Process did not exit within timeout, killing",
                        self.entry_id
                    );
                    let _ = child.kill().await;
                }
            }
        }

        self.stream = None;

        tracing::info!("[{}] Sandbox stopped", self.entry_id);

        Ok(())
    }
}
