use std::error::Error;
use std::time::Duration;

use async_trait::async_trait;
use rumqttc::AsyncClient;
use rumqttc::Event;
use rumqttc::MqttOptions;
use rumqttc::Packet;
use rumqttc::QoS;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing;

/// MQTT message received from a subscription
#[derive(Debug, Clone)]
pub struct MqttMessage {
    pub topic: String,
    pub payload: Vec<u8>,
    #[allow(dead_code)]
    pub retain: bool,
}

/// Trait for MQTT client operations
///
/// This trait allows for mocking the MQTT client for testing purposes
#[async_trait]
pub trait MqttClient: Send + Sync {
    /// Connect to the MQTT broker
    async fn connect(&mut self) -> Result<(), Box<dyn Error + Send>>;

    /// Subscribe to an MQTT topic
    async fn subscribe(&mut self, topic: &str) -> Result<(), Box<dyn Error + Send>>;

    /// Publish a message to an MQTT topic
    async fn publish(
        &mut self,
        topic: &str,
        payload: &[u8],
        retain: bool,
    ) -> Result<(), Box<dyn Error + Send>>;

    /// Poll for the next message from subscribed topics
    ///
    /// Returns None if no message is available or if the client should stop
    async fn poll_message(&mut self) -> Option<MqttMessage>;
}

/// Mock MQTT client for testing
#[cfg(test)]
#[derive(Debug, Default)]
pub struct MockMqttClient {
    pub messages: Vec<MqttMessage>,
    pub subscriptions: Vec<String>,
    pub published: Vec<(String, Vec<u8>, bool)>,
    pub is_connected: bool,
}

#[cfg(test)]
#[async_trait]
impl MqttClient for MockMqttClient {
    async fn connect(&mut self) -> Result<(), Box<dyn Error + Send>> {
        self.is_connected = true;
        Ok(())
    }

    async fn subscribe(&mut self, topic: &str) -> Result<(), Box<dyn Error + Send>> {
        self.subscriptions.push(topic.to_string());
        Ok(())
    }

    async fn publish(
        &mut self,
        topic: &str,
        payload: &[u8],
        retain: bool,
    ) -> Result<(), Box<dyn Error + Send>> {
        self.published
            .push((topic.to_string(), payload.to_vec(), retain));
        Ok(())
    }

    async fn poll_message(&mut self) -> Option<MqttMessage> {
        self.messages.pop()
    }
}

#[cfg(test)]
impl MockMqttClient {
    /// Create a new mock MQTT client
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a message to the mock client's queue
    #[allow(dead_code)]
    pub fn add_message(&mut self, topic: String, payload: Vec<u8>, retain: bool) {
        self.messages.push(MqttMessage {
            topic,
            payload,
            retain,
        });
    }
}

/// Real MQTT client implementation using rumqttc
pub struct RumqttcClient {
    /// MQTT connection options (stored for lazy initialization)
    mqtt_options: MqttOptions,

    /// AsyncClient (created in connect())
    client: Option<AsyncClient>,

    /// Message receiver (created in connect())
    message_rx: Option<mpsc::UnboundedReceiver<MqttMessage>>,

    /// Background event loop task handle
    event_loop_task: Option<JoinHandle<()>>,
}

impl RumqttcClient {
    /// Create a new RumqttcClient from configuration
    pub fn new(config: &crate::integrations::mqtt::MqttConfig) -> anyhow::Result<Self> {
        let mut mqtt_options =
            MqttOptions::new(config.client_id.clone(), config.broker.clone(), config.port);

        // Set keep-alive interval
        mqtt_options.set_keep_alive(Duration::from_secs(30));

        // Allow large MQTT packets (2 MiB) for discovery payloads
        mqtt_options.set_max_packet_size(2 * 1024 * 1024, 2 * 1024 * 1024);

        // Set credentials if provided
        if let (Some(username), Some(password)) = (&config.username, &config.password) {
            mqtt_options.set_credentials(username, password);
        }

        Ok(Self {
            mqtt_options,
            client: None,
            message_rx: None,
            event_loop_task: None,
        })
    }
}

#[async_trait]
impl MqttClient for RumqttcClient {
    async fn connect(&mut self) -> Result<(), Box<dyn Error + Send>> {
        // Create client and event loop
        let (client, mut event_loop) = AsyncClient::new(self.mqtt_options.clone(), 10);

        // Create channel for messages
        let (message_tx, message_rx) = mpsc::unbounded_channel();

        // Spawn background task to poll event loop
        let task = tokio::spawn(async move {
            loop {
                match event_loop.poll().await {
                    Ok(Event::Incoming(Packet::Publish(publish))) => {
                        let msg = MqttMessage {
                            topic: publish.topic.to_string(),
                            payload: publish.payload.to_vec(),
                            retain: publish.retain,
                        };

                        // Send to channel; if receiver dropped, exit
                        if message_tx.send(msg).is_err() {
                            break;
                        }
                    }
                    Ok(_) => {
                        // Ignore other events (connack, puback, etc.)
                    }
                    Err(e) => {
                        tracing::warn!("MQTT event loop error: {}", e);
                        // Sleep briefly before retrying
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
            tracing::info!("MQTT event loop task exiting");
        });

        self.client = Some(client);
        self.message_rx = Some(message_rx);
        self.event_loop_task = Some(task);

        Ok(())
    }

    async fn subscribe(&mut self, topic: &str) -> Result<(), Box<dyn Error + Send>> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| -> Box<dyn Error + Send> {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::NotConnected,
                    "MQTT client not connected. Call connect() first.",
                ))
            })?;

        client
            .subscribe(topic, QoS::AtMostOnce)
            .await
            .map_err(|e| Box::new(e) as Box<dyn Error + Send>)?;

        Ok(())
    }

    async fn publish(
        &mut self,
        topic: &str,
        payload: &[u8],
        retain: bool,
    ) -> Result<(), Box<dyn Error + Send>> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| -> Box<dyn Error + Send> {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::NotConnected,
                    "MQTT client not connected. Call connect() first.",
                ))
            })?;

        client
            .publish(topic, QoS::AtLeastOnce, retain, payload)
            .await
            .map_err(|e| Box::new(e) as Box<dyn Error + Send>)?;

        Ok(())
    }

    async fn poll_message(&mut self) -> Option<MqttMessage> {
        match &mut self.message_rx {
            Some(rx) => rx.recv().await,
            None => None,
        }
    }
}

impl Drop for RumqttcClient {
    fn drop(&mut self) {
        if let Some(task) = self.event_loop_task.take() {
            task.abort();
        }
    }
}
