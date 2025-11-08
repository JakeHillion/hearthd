//! Sandbox management for running Python integrations.
//!
//! Uses tokio::net::UnixStream::pair() to create a socketpair and passes
//! the file descriptor to the Python process via environment variable.
//! No filesystem paths are used.

use super::protocol::{Message, Response};
use super::Error;
use super::Result;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::{Child, Command};

use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::process::Stdio;

#[derive(Debug)]
pub struct SandboxBuilder {
    /// Entry ID for this integration instance
    pub name: String,

    /// Path to the Python executable
    pub python_path: PathBuf,

    /// Path to Home Assistant source (for integrations)
    pub ha_source_path: PathBuf,
}

impl SandboxBuilder {
    // TODO: can we implement `TryFrom<SandboxBuilder> for Sandbox` but async?
    pub async fn try_into_sandbox(&self) -> Result<Sandbox> {
        tracing::info!("[{}] Starting sandbox", self.name);

        // Create socketpair for bidirectional communication
        let (rust_stream, python_stream) = UnixStream::pair()?;

        // Get file descriptor number for Python side
        let python_fd = python_stream.as_raw_fd();
        tracing::debug!(
            "[{}] Created socketpair, Python FD: {}",
            self.name,
            python_fd
        );

        // Build command to spawn Python runner
        let mut cmd = Command::new(&self.python_path);
        cmd.arg("-u") // Unbuffered output
            .arg("python/runner.py")
            .env("HEARTHD_SOCKET_FD", python_fd.to_string())
            .env("HEARTHD_name", &self.name)
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
            tracing::error!("[{}] Failed to spawn Python process: {}", self.name, e);
            e
        })?;

        tracing::info!(
            "[{}] Python process spawned (PID: {})",
            self.name,
            child.id().unwrap_or(0)
        );

        // Capture stdout/stderr for logging
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let name_clone = self.name.clone();

        // Spawn tasks to log Python output
        if let Some(mut stdout) = stdout {
            let name = name_clone.clone();
            tokio::spawn(async move {
                use tokio::io::AsyncBufReadExt;
                let reader = tokio::io::BufReader::new(&mut stdout);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::info!("[{}] [stdout] {}", name, line);
                }
            });
        }

        if let Some(mut stderr) = stderr {
            let name = name_clone;
            tokio::spawn(async move {
                use tokio::io::AsyncBufReadExt;
                let reader = tokio::io::BufReader::new(&mut stderr);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::warn!("[{}] [stderr] {}", name, line);
                }
            });
        }

        // Drop Python side socket (child owns it now)
        drop(python_stream);

        tracing::debug!("[{}] Sandbox started, waiting for Ready message", self.name);

        Ok(Sandbox {
            name: self.name.clone(),
            stream: BufReader::new(rust_stream),
            child,
        })
    }

    pub fn new(name: String, python_path: PathBuf, ha_source_path: PathBuf) -> Self {
        Self {
            name,
            python_path,
            ha_source_path,
        }
    }
}

/// Manages a sandboxed Python environment for running Home Assistant integrations.
#[derive(Debug)]
pub struct Sandbox {
    name: String,

    /// Tokio Unix stream for communication with Python
    stream: BufReader<UnixStream>,

    /// Child process handle
    child: Child,
}

impl Sandbox {
    /// Send a response to the Python process
    pub async fn send(&mut self, response: Response) -> Result<()> {
        // Serialize to JSON
        let json = serde_json::to_string(&response)?;

        tracing::trace!("[{}] Sending: {}", self.name, json);

        // Write JSON + newline
        self.stream.write_all(json.as_bytes()).await?;
        self.stream.write_all(b"\n").await?;
        self.stream.flush().await?;

        Ok(())
    }

    /// Receive a message from the Python process
    pub async fn recv(&mut self) -> Result<Message> {
        // Read line (newline-delimited JSON)
        let mut line = String::new();
        self.stream.read_line(&mut line).await?;

        if line.is_empty() {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "Socket closed",
            )));
        }

        tracing::trace!("[{}] Received: {}", self.name, line.trim());

        // Deserialize from JSON
        let message: Message = serde_json::from_str(line.trim())?;

        Ok(message)
    }
}
