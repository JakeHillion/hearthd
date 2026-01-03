use super::protocol::Message;
use super::protocol::Response;
use super::Error;
use super::Result;
use super::Sandbox;

use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::debug;
use tracing::error;
use tracing::info;
use tracing::warn;

/// Configuration for setting up an integration.
#[derive(Debug, Clone)]
pub struct IntegrationConfig {
    pub domain: String,
    pub name: String,
    pub config: serde_json::Value,
}

impl Default for IntegrationConfig {
    fn default() -> Self {
        // Default to met integration with Oslo coordinates for MVP
        Self {
            domain: "met".into(),
            name: "met_oslo".into(),
            config: serde_json::json!({
                "latitude": 59.9139,
                "longitude": 10.7522,
                "elevation": 23,
                "track_home": false,
                "name": "Oslo Weather"
            }),
        }
    }
}

/// Event from a timer to trigger an update.
struct TimerEvent {
    timer_id: String,
    name: String,
}

pub(super) struct Integration {
    sandbox: Sandbox,
    state: State,
    config: IntegrationConfig,
    platforms: Vec<String>,
    /// Timer task handles, keyed by timer_id
    timer_handles: HashMap<String, JoinHandle<()>>,
    /// Channel for receiving timer events
    timer_tx: mpsc::Sender<TimerEvent>,
    timer_rx: mpsc::Receiver<TimerEvent>,
}

impl std::fmt::Debug for Integration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Integration")
            .field("state", &self.state)
            .field("config", &self.config)
            .field("platforms", &self.platforms)
            .field("timer_count", &self.timer_handles.len())
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
enum State {
    NotStarted,
    AwaitingSetupStatus,
    Running,
}

impl Integration {
    pub fn new(sandbox: Sandbox) -> Self {
        Self::with_config(sandbox, IntegrationConfig::default())
    }

    pub fn with_config(sandbox: Sandbox, config: IntegrationConfig) -> Self {
        // Create channel for timer events
        let (timer_tx, timer_rx) = mpsc::channel(16);

        Self {
            sandbox,
            state: State::NotStarted,
            config,
            platforms: Vec::new(),
            timer_handles: HashMap::new(),
            timer_tx,
            timer_rx,
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        // State machine:
        // 1. Python sends "Ready" message.
        // 2. We send "SetupIntegration" message with config.
        // 3. Python sends "SetupComplete" or "SetupFailed".
        // 4. Enter Running state and handle ongoing messages.
        loop {
            match self.state {
                State::NotStarted => {
                    self.handle_not_started().await?;
                }
                State::AwaitingSetupStatus => {
                    self.handle_awaiting_setup_status().await?;
                }
                State::Running => {
                    self.handle_running().await?;
                }
            }
        }
    }

    async fn handle_not_started(&mut self) -> Result<()> {
        // Expect the integration to say "Ready". Then send back the SetupIntegration message.
        match self.sandbox.recv().await? {
            Message::Ready => {
                info!(
                    "[{}] Received Ready, sending SetupIntegration for domain '{}'",
                    self.config.name, self.config.domain
                );
                self.sandbox
                    .send(Response::SetupIntegration {
                        domain: self.config.domain.clone(),
                        name: self.config.name.clone(),
                        config: self.config.config.clone(),
                    })
                    .await?;
                self.state = State::AwaitingSetupStatus;
                Ok(())
            }
            m => Err(Error::InvalidMessage {
                expected: "Ready".into(),
                received: m,
            }),
        }
    }

    async fn handle_awaiting_setup_status(&mut self) -> Result<()> {
        loop {
            match self.sandbox.recv().await? {
                Message::SetupComplete { name, platforms } => {
                    info!(
                        "[{}] SetupComplete with platforms: {:?}",
                        name, platforms
                    );
                    self.platforms = platforms;
                    self.state = State::Running;
                    return Ok(());
                }
                Message::SetupFailed {
                    name,
                    error,
                    error_type,
                    missing_package,
                } => {
                    error!(
                        "[{}] SetupFailed: {} (type: {:?}, missing: {:?})",
                        name, error, error_type, missing_package
                    );
                    return Err(Error::SetupFailed {
                        name,
                        error,
                        error_type,
                        missing_package,
                    });
                }
                Message::ScheduleUpdate {
                    timer_id,
                    name,
                    interval_seconds,
                } => {
                    // Schedule coordinator updates even during setup
                    info!(
                        "[{}] ScheduleUpdate during setup: timer_id={} name={} interval={}s",
                        self.config.name, timer_id, name, interval_seconds
                    );
                    self.schedule_timer(timer_id, name, interval_seconds);
                }
                m => {
                    warn!(
                        "[{}] Unexpected message during setup (ignoring): {:?}",
                        self.config.name, m
                    );
                }
            }
        }
    }

    async fn handle_running(&mut self) -> Result<()> {
        // Use select to handle both sandbox messages and timer events
        tokio::select! {
            // Handle messages from Python sandbox
            msg_result = self.sandbox.recv() => {
                let msg = msg_result?;
                self.handle_sandbox_message(msg).await?;
            }
            // Handle timer trigger events
            Some(timer_event) = self.timer_rx.recv() => {
                self.handle_timer_event(timer_event).await?;
            }
        }
        Ok(())
    }

    async fn handle_timer_event(&mut self, event: TimerEvent) -> Result<()> {
        debug!(
            "[{}] Timer triggered: {} ({})",
            self.config.name, event.timer_id, event.name
        );
        self.sandbox
            .send(Response::TriggerUpdate {
                timer_id: event.timer_id,
                name: event.name,
            })
            .await
    }

    async fn handle_sandbox_message(&mut self, msg: Message) -> Result<()> {
        match msg {
            Message::EntityRegister {
                name,
                entity_id,
                platform,
                device_class,
                capabilities,
                device_info,
            } => {
                info!(
                    "[{}] EntityRegister: {} ({}) platform={} device_class={:?}",
                    self.config.name, entity_id, name, platform, device_class
                );
                debug!(
                    "  capabilities={:?} device_info={:?}",
                    capabilities, device_info
                );
                // TODO: Forward to engine's entity registry
            }

            Message::StateUpdate {
                entity_id,
                state,
                attributes,
                last_updated,
            } => {
                info!(
                    "[{}] StateUpdate: {} = {} (updated: {})",
                    self.config.name, entity_id, state, last_updated
                );
                debug!("  attributes={}", attributes);
                // TODO: Forward to engine's state registry
            }

            Message::ScheduleUpdate {
                timer_id,
                name,
                interval_seconds,
            } => {
                info!(
                    "[{}] ScheduleUpdate: timer_id={} name={} interval={}s",
                    self.config.name, timer_id, name, interval_seconds
                );
                self.schedule_timer(timer_id, name, interval_seconds);
            }

            Message::CancelTimer { timer_id } => {
                info!("[{}] CancelTimer: {}", self.config.name, timer_id);
                self.cancel_timer(&timer_id);
            }

            Message::GetConfig { request_id, keys } => {
                debug!(
                    "[{}] GetConfig: request_id={} keys={:?}",
                    self.config.name, request_id, keys
                );
                // Return empty config for now
                self.sandbox
                    .send(Response::ConfigResponse {
                        request_id,
                        config: HashMap::new(),
                    })
                    .await?;
            }

            Message::Log {
                level,
                logger,
                message,
            } => {
                // Forward Python logs to Rust tracing
                match level {
                    super::protocol::LogLevel::Debug => {
                        debug!("[{}] [{}] {}", self.config.name, logger, message)
                    }
                    super::protocol::LogLevel::Info => {
                        info!("[{}] [{}] {}", self.config.name, logger, message)
                    }
                    super::protocol::LogLevel::Warning => {
                        warn!("[{}] [{}] {}", self.config.name, logger, message)
                    }
                    super::protocol::LogLevel::Error => {
                        error!("[{}] [{}] {}", self.config.name, logger, message)
                    }
                }
            }

            Message::UpdateComplete {
                timer_id,
                success,
                error,
            } => {
                if success {
                    debug!(
                        "[{}] UpdateComplete: timer_id={} success",
                        self.config.name, timer_id
                    );
                } else {
                    warn!(
                        "[{}] UpdateComplete: timer_id={} failed: {:?}",
                        self.config.name, timer_id, error
                    );
                }
            }

            Message::UnloadComplete { name } => {
                info!("[{}] UnloadComplete", name);
                // Integration has unloaded, we could exit the loop here
            }

            // These messages shouldn't appear in Running state
            Message::Ready => {
                warn!("[{}] Unexpected Ready message in Running state", self.config.name);
            }
            Message::SetupComplete { .. } => {
                warn!(
                    "[{}] Unexpected SetupComplete message in Running state",
                    self.config.name
                );
            }
            Message::SetupFailed { .. } => {
                warn!(
                    "[{}] Unexpected SetupFailed message in Running state",
                    self.config.name
                );
            }
        }
        Ok(())
    }

    /// Schedule a timer to trigger coordinator updates.
    fn schedule_timer(&mut self, timer_id: String, name: String, interval_seconds: u64) {
        // Cancel existing timer with same ID if present
        self.cancel_timer(&timer_id);

        let tx = self.timer_tx.clone();
        let timer_id_clone = timer_id.clone();
        let name_clone = name.clone();
        let interval = Duration::from_secs(interval_seconds);

        let handle = tokio::spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);
            // Skip the first immediate tick
            interval_timer.tick().await;

            loop {
                interval_timer.tick().await;
                let event = TimerEvent {
                    timer_id: timer_id_clone.clone(),
                    name: name_clone.clone(),
                };
                if tx.send(event).await.is_err() {
                    // Channel closed, stop the timer
                    break;
                }
            }
        });

        self.timer_handles.insert(timer_id, handle);
    }

    /// Cancel a scheduled timer.
    fn cancel_timer(&mut self, timer_id: &str) {
        if let Some(handle) = self.timer_handles.remove(timer_id) {
            handle.abort();
            debug!("[{}] Timer {} cancelled", self.config.name, timer_id);
        }
    }
}
