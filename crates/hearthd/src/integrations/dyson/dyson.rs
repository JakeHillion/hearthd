//! Dyson Pure Cool integration implementation.
//!
//! The integration runs a background MQTT session per device and emits state
//! updates into the engine. Commands from the engine are translated into Dyson
//! `STATE-SET` messages and published to the device.

use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::config::Config;
use super::config::DeviceConfig;
use super::mapping;
use super::mapping::build_endpoints;
use super::mapping::node_for_device;
use super::mapping::state_set_payload;
use super::state::PureCoolState;
use super::transport::CommandPayload;
use super::transport::MqttSession;
use super::transport::TransportMessage;
use crate::engine::FromIntegrationMessage;
use crate::engine::FromIntegrationSender;
use crate::engine::Integration;
use crate::engine::NodeId;
use crate::engine::NodeIdAllocator;
use crate::engine::ToIntegrationMessage;
use crate::matter::EndpointId;
use crate::matter::Node;

const ENVIRONMENTAL_POLL_INTERVAL: Duration = Duration::from_secs(30);
const SUPPORTED_DEVICE_TYPE: &str = "438";

/// Per-device runtime state shared between the integration task and the
/// background transport loop.
struct Device {
    config: DeviceConfig,
    node_id: NodeId,
    state: PureCoolState,
    /// Channel for the integration task to send commands to the transport task.
    command_tx: mpsc::Sender<CommandPayload>,
}

/// Dyson Pure Cool integration.
pub struct DysonIntegration {
    config: Config,
    tx: Option<FromIntegrationSender>,
    inner: Arc<Mutex<Inner>>,
    _tasks: Vec<JoinHandle<()>>,
}

struct Inner {
    devices: HashMap<String, Device>,
}

impl DysonIntegration {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            tx: None,
            inner: Arc::new(Mutex::new(Inner {
                devices: HashMap::new(),
            })),
            _tasks: Vec::new(),
        }
    }

    fn validate_device(name: &str, device: &DeviceConfig) -> Result<(), anyhow::Error> {
        if device.device_type != SUPPORTED_DEVICE_TYPE {
            anyhow::bail!(
                "dyson device '{}' has unsupported device_type '{}' (only '{}' is supported)",
                name,
                device.device_type,
                SUPPORTED_DEVICE_TYPE
            );
        }
        Ok(())
    }

    async fn publish_all_state(
        tx: &FromIntegrationSender,
        node_id: NodeId,
        state: &PureCoolState,
    ) -> Result<(), Box<dyn Error + Send>> {
        let endpoints = build_endpoints(state);
        for (endpoint_id, endpoint) in &endpoints {
            for cluster in endpoint.clusters.values() {
                tx.send(FromIntegrationMessage::AttributeChanged {
                    node_id,
                    endpoint_id: *endpoint_id,
                    cluster: cluster.clone(),
                })
                .await
                .map_err(|e| Box::new(e) as Box<dyn Error + Send>)?;
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Integration for DysonIntegration {
    fn name(&self) -> &str {
        "dyson"
    }

    async fn setup(
        &mut self,
        tx: FromIntegrationSender,
        node_ids: NodeIdAllocator,
    ) -> Result<(), Box<dyn Error + Send>> {
        self.tx = Some(tx.clone());
        let mut inner = self.inner.lock().await;

        for (name, device) in &self.config.devices {
            Self::validate_device(name, device)?;

            let node_id = node_ids.allocate();
            let node: Node = node_for_device(name, device);
            tx.send(FromIntegrationMessage::NodeAdded { node_id, node })
                .await
                .map_err(|e| Box::new(e) as Box<dyn Error + Send>)?;

            let session = MqttSession::connect(
                &device.host,
                &device.serial,
                &device.credential,
                &device.device_type,
            )
            .await
            .map_err(|e| -> Box<dyn Error + Send> {
                Box::new(std::io::Error::other(e.to_string()))
            })?;

            let command_tx = session.command_tx.clone();
            inner.devices.insert(
                name.clone(),
                Device {
                    config: device.clone(),
                    node_id,
                    state: PureCoolState::default(),
                    command_tx,
                },
            );

            let inner_clone = self.inner.clone();
            let to_engine = tx.clone();
            let name_for_task = name.clone();
            let device_for_task = device.clone();

            let transport_task = tokio::spawn(async move {
                run_transport_loop(
                    session,
                    inner_clone,
                    to_engine,
                    name_for_task,
                    device_for_task,
                )
                .await;
            });
            self._tasks.push(transport_task);
        }

        Ok(())
    }

    async fn handle_message(
        &mut self,
        msg: ToIntegrationMessage,
    ) -> Result<(), Box<dyn Error + Send>> {
        let ToIntegrationMessage::InvokeCommand {
            node_id,
            endpoint_id,
            command,
        } = msg;

        let inner = self.inner.lock().await;
        let device = inner
            .devices
            .iter()
            .find(|(_, d)| d.node_id == node_id)
            .map(|(_, d)| d)
            .ok_or_else(|| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("no Dyson device for node_id {:?}", node_id),
                )) as Box<dyn Error + Send>
            })?;
        let config = device.config.clone();
        let node_id = device.node_id;
        let command_tx = device.command_tx.clone();
        drop(inner);

        let payload = state_set_payload(endpoint_id, &command).ok_or_else(|| {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "unsupported command for Dyson",
            )) as Box<dyn Error + Send>
        })?;

        // Forward the command to the transport task, which owns the MQTT
        // session and publishes it. Failure here means the transport has gone
        // away, which is a real problem worth surfacing.
        command_tx.send(payload).await.map_err(|_| {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "Dyson transport task is gone",
            )) as Box<dyn Error + Send>
        })?;

        // Optimistically update local state so the UI reflects the command
        // immediately; the next device report confirms or corrects it.
        let mut inner = self.inner.lock().await;
        let device = inner.devices.get_mut(&config.serial).ok_or_else(|| {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "device disappeared during command handling",
            )) as Box<dyn Error + Send>
        })?;
        let _ = apply_command_to_state(&mut device.state, endpoint_id, &command);
        let tx = self.tx.as_ref().expect("tx set in setup");
        DysonIntegration::publish_all_state(tx, node_id, &device.state).await?;

        Ok(())
    }

    async fn shutdown(&mut self) -> Result<(), Box<dyn Error + Send>> {
        for task in self._tasks.drain(..) {
            task.abort();
        }
        Ok(())
    }
}

async fn run_transport_loop(
    mut session: MqttSession,
    inner: Arc<Mutex<Inner>>,
    to_engine: FromIntegrationSender,
    name: String,
    device: DeviceConfig,
) {
    let mut interval = tokio::time::interval(ENVIRONMENTAL_POLL_INTERVAL);

    loop {
        tokio::select! {
            _ = interval.tick() => {
                let _ = session
                    .request_environmental(&device.device_type, &device.serial)
                    .await;
            }
            Some(msg) = session.events.recv() => {
                match msg {
                    TransportMessage::Connected => {
                        let _ = session
                            .request_state(&device.device_type, &device.serial)
                            .await;
                    }
                    TransportMessage::Disconnected => {}
                    TransportMessage::State(value) => {
                        let mut guard = inner.lock().await;
                        let Some(dev) = guard.devices.get_mut(&name) else {
                            continue;
                        };
                        dev.state.apply_state_payload(&value);
                        let node_id = dev.node_id;
                        let state = dev.state.clone();
                        drop(guard);
                        if let Err(e) = DysonIntegration::publish_all_state(
                            &to_engine,
                            node_id,
                            &state,
                        )
                        .await
                        {
                            tracing::warn!("failed to publish Dyson state: {}", e);
                        }
                    }
                    TransportMessage::Environmental(value) => {
                        let mut guard = inner.lock().await;
                        let Some(dev) = guard.devices.get_mut(&name) else {
                            continue;
                        };
                        dev.state.apply_environmental_payload(&value);
                        let node_id = dev.node_id;
                        let state = dev.state.clone();
                        drop(guard);
                        if let Err(e) = DysonIntegration::publish_all_state(
                            &to_engine,
                            node_id,
                            &state,
                        )
                        .await
                        {
                            tracing::warn!("failed to publish Dyson environmental state: {}", e);
                        }
                    }
                }
            }
            Some(command) = session.command_rx.recv() => {
                if let Err(e) = session
                    .publish_command(&device.device_type, &device.serial, command)
                    .await
                {
                    tracing::warn!("failed to publish Dyson command: {}", e);
                }
            }
        }
    }
}

fn apply_command_to_state(
    state: &mut PureCoolState,
    endpoint_id: EndpointId,
    command: &crate::matter::ClusterCommand,
) -> Option<()> {
    use crate::matter::AirflowDirection;
    use crate::matter::ClusterCommand;
    use crate::matter::FanControlCommand;
    use crate::matter::OnOffCommand;

    match command {
        ClusterCommand::OnOff(cmd) => match cmd {
            OnOffCommand::On => state.fan_power = Some(true),
            OnOffCommand::Off => state.fan_power = Some(false),
            OnOffCommand::Toggle => {}
        },
        ClusterCommand::FanControl(cmd) => match cmd {
            FanControlCommand::SetFanMode { mode } => match mode {
                crate::matter::FanMode::Off => {
                    state.fan_power = Some(false);
                    state.auto_mode = Some(false);
                }
                crate::matter::FanMode::Auto => {
                    state.fan_power = Some(true);
                    state.auto_mode = Some(true);
                    state.fan_speed = None;
                }
                _ => {
                    state.fan_power = Some(true);
                    state.auto_mode = Some(false);
                    if state.fan_speed.is_none() {
                        state.fan_speed = Some(5);
                    }
                }
            },
            FanControlCommand::SetPercentSetting { percent } => {
                let speed = (*percent / 10).clamp(1, 10);
                state.fan_power = Some(speed != 0);
                state.auto_mode = Some(false);
                state.fan_speed = Some(speed);
            }
            FanControlCommand::SetSpeedSetting { speed } => {
                let speed = (*speed).clamp(0, 10);
                state.fan_power = Some(speed != 0);
                state.auto_mode = Some(false);
                state.fan_speed = Some(speed);
            }
            FanControlCommand::SetAirflowDirection { direction } => {
                state.front_airflow = Some(matches!(direction, AirflowDirection::Forward));
            }
        },
        ClusterCommand::ModeSelect(crate::matter::ModeSelectCommand::ChangeToMode { new_mode }) => {
            let on = *new_mode == 1;
            match endpoint_id {
                mapping::EP_OSCILLATION => state.oscillation = Some(on),
                mapping::EP_NIGHT_MODE => state.night_mode = Some(on),
                mapping::EP_MONITORING => state.continuous_monitoring = Some(on),
                _ => {}
            }
        }
        ClusterCommand::CountdownTimer(crate::matter::CountdownTimerCommand::SetCountdown {
            seconds,
        }) => {
            state.sleep_timer = Some(if *seconds == 0 {
                0
            } else {
                ((seconds / 60) as u16).clamp(1, 540)
            });
        }
        _ => return None,
    }
    Some(())
}
