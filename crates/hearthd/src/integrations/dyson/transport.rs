//! MQTT transport for a single Dyson device.
//!
//! Connects to the device's local MQTT broker using the serial number as the
//! username and the cloud-derived credential as the password. Subscribes to
//! the status topic and forwards incoming messages to the integration.

use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use rumqttc::AsyncClient;
use rumqttc::Event as MqttEvent;
use rumqttc::Incoming;
use rumqttc::MqttOptions;
use rumqttc::Packet;
use rumqttc::QoS;
use serde_json::Value;
use tokio::sync::mpsc;

const KEEP_ALIVE_SECONDS: u64 = 60;
const STATUS_TOPIC_SUFFIX: &str = "/status/current";

/// A `STATE-SET` command forwarded from the engine to the transport task.
pub type CommandPayload = serde_json::Map<String, Value>;

/// Message produced by the transport for the integration task.
#[derive(Debug)]
pub enum TransportMessage {
    Connected,
    Disconnected,
    State(Value),
    Environmental(Value),
}

/// Handle to an active MQTT session.
///
/// The MQTT `EventLoop`, and thus the sole owner that can both read incoming
/// messages and publish, runs in a background task spawned by [`MqttSession`].
/// This struct is the channel harness for that task: `events` yields messages
/// from the device, and `command_tx` is how the integration asks the task to
/// publish a `STATE-SET` command. The task reads from `command_rx`.
pub struct MqttSession {
    pub client: AsyncClient,
    /// Messages received from the device.
    pub events: mpsc::Receiver<TransportMessage>,
    /// Commands queued by `handle_message` for the transport task to publish.
    pub command_rx: mpsc::Receiver<CommandPayload>,
    /// Sender half of `command_rx`, held by the integration and the daemon.
    pub command_tx: mpsc::Sender<CommandPayload>,
}

impl MqttSession {
    pub async fn connect(
        host: &str,
        serial: &str,
        credential: &str,
        device_type: &str,
    ) -> Result<Self> {
        let mut options = MqttOptions::new(serial, host, 1883);
        options.set_keep_alive(Duration::from_secs(KEEP_ALIVE_SECONDS));
        options.set_credentials(serial, credential);
        options.set_clean_session(true);

        let (client, mut eventloop) = AsyncClient::new(options, 128);
        let status_topic = format!(
            "{}{}",
            status_topic_prefix(device_type, serial),
            STATUS_TOPIC_SUFFIX
        );
        client
            .subscribe(&status_topic, QoS::AtLeastOnce)
            .await
            .context("failed to subscribe to Dyson status topic")?;

        let (tx, rx) = mpsc::channel(64);
        let (command_tx, command_rx) = mpsc::channel(64);
        let serial = serial.to_string();

        tokio::spawn(async move {
            let mut connected = false;
            loop {
                match eventloop.poll().await {
                    Ok(MqttEvent::Incoming(Incoming::ConnAck(_))) => {
                        connected = true;
                        let _ = tx.send(TransportMessage::Connected).await;
                    }
                    Ok(MqttEvent::Incoming(Packet::Publish(publish))) => {
                        if let Ok(value) = serde_json::from_slice::<Value>(&publish.payload) {
                            match classify_message(&value) {
                                Some(MessageKind::State) => {
                                    let _ = tx.send(TransportMessage::State(value)).await;
                                }
                                Some(MessageKind::Environmental) => {
                                    let _ = tx.send(TransportMessage::Environmental(value)).await;
                                }
                                None => {}
                            }
                        }
                    }
                    Ok(MqttEvent::Incoming(Incoming::Disconnect)) => {
                        connected = false;
                        let _ = tx.send(TransportMessage::Disconnected).await;
                    }
                    Err(err) => {
                        tracing::warn!(serial, "Dyson MQTT error: {}", err);
                        if connected {
                            connected = false;
                            let _ = tx.send(TransportMessage::Disconnected).await;
                        }
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                    _ => {}
                }
            }
        });

        Ok(Self {
            client,
            events: rx,
            command_rx,
            command_tx,
        })
    }

    /// Publish a `STATE-SET` command to the device.
    pub async fn publish_command(
        &self,
        device_type: &str,
        serial: &str,
        payload: serde_json::Map<String, Value>,
    ) -> Result<()> {
        let topic = format!("{}/command", status_topic_prefix(device_type, serial));
        self.client
            .publish(
                topic,
                QoS::AtLeastOnce,
                false,
                serde_json::to_vec(&payload)?,
            )
            .await
            .context("failed to publish Dyson command")
    }

    /// Publish a request for current state.
    pub async fn request_state(&self, device_type: &str, serial: &str) -> Result<()> {
        let payload = serde_json::json!({
            "msg": "REQUEST-CURRENT-STATE",
        });
        let topic = format!("{}/command", status_topic_prefix(device_type, serial));
        self.client
            .publish(
                topic,
                QoS::AtLeastOnce,
                false,
                serde_json::to_vec(&payload)?,
            )
            .await
            .context("failed to request Dyson state")
    }

    /// Publish a request for environmental data.
    pub async fn request_environmental(&self, device_type: &str, serial: &str) -> Result<()> {
        let payload = serde_json::json!({
            "msg": "REQUEST-PRODUCT-ENVIRONMENT-CURRENT-SENSOR-DATA",
        });
        let topic = format!("{}/command", status_topic_prefix(device_type, serial));
        self.client
            .publish(
                topic,
                QoS::AtLeastOnce,
                false,
                serde_json::to_vec(&payload)?,
            )
            .await
            .context("failed to request Dyson environmental data")
    }
}

fn status_topic_prefix(device_type: &str, serial: &str) -> String {
    format!("{}/{}", device_type, serial)
}

#[derive(Debug, Clone, Copy)]
enum MessageKind {
    State,
    Environmental,
}

fn classify_message(value: &Value) -> Option<MessageKind> {
    let msg = value.get("msg").and_then(|v| v.as_str())?;
    match msg {
        "CURRENT-STATE" | "STATE-CHANGE" => Some(MessageKind::State),
        "ENVIRONMENTAL-CURRENT-SENSOR-DATA" => Some(MessageKind::Environmental),
        _ => None,
    }
}
