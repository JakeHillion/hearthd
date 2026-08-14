//! The MQTT transport carrying Wave 3 frames.
//!
//! Nothing here is derived from the reverse-engineering work: it is an MQTT
//! client configured with the parameters recorded in `super::topics` and
//! `super::auth`. The trait exists so the integration can be driven from tests
//! without a broker.
//!
//! TLS certificate verification is mandatory and not configurable. The
//! credentials are bearer-style — anyone who can intercept them controls every
//! device on the account — so an escape hatch for self-signed certificates
//! would be a footgun with no legitimate use against EcoFlow's own broker.

use std::time::Duration;

use async_trait::async_trait;
use rumqttc::AsyncClient;
use rumqttc::Event;
use rumqttc::MqttOptions;
use rumqttc::Packet;
use rumqttc::QoS;
use rumqttc::SubscribeReasonCode;
use rumqttc::TlsConfiguration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::auth::MqttCredentials;

/// The broker drops idle sessions readily, so the keepalive is short.
const KEEPALIVE: Duration = Duration::from_secs(15);

/// Both directions use QoS 1: commands must not be silently lost, and
/// telemetry gaps would show up as stale state.
const QOS: QoS = QoS::AtLeastOnce;

/// A message received from a subscribed topic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub topic: String,
    pub payload: Vec<u8>,
}

#[derive(Debug)]
pub struct TransportError(pub String);

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for TransportError {}

/// The receiving half of a session.
///
/// Handed out by `Transport::connect` rather than being reachable through the
/// transport itself, and deliberately so: waiting for the next message blocks
/// for as long as the device stays quiet, which with a shared transport would
/// hold a lock that publishing also needs. Keeping the two halves separate
/// means a command can go out while the receive loop is parked.
pub struct MessageStream {
    messages: mpsc::UnboundedReceiver<Message>,
}

impl MessageStream {
    /// Wrap the receiving end of a session's message channel.
    pub fn new(messages: mpsc::UnboundedReceiver<Message>) -> Self {
        Self { messages }
    }

    /// Wait for the next message. `None` means the session has ended and the
    /// caller should reconnect.
    pub async fn next(&mut self) -> Option<Message> {
        self.messages.recv().await
    }
}

/// MQTT operations the integration needs.
#[async_trait]
pub trait Transport: Send + Sync {
    /// Open a session, returning its receiving half.
    ///
    /// Sessions are clean, so subscriptions do not survive a reconnect and the
    /// caller must re-subscribe every time.
    async fn connect(
        &mut self,
        credentials: &MqttCredentials,
        client_id: &str,
    ) -> Result<MessageStream, TransportError>;

    async fn subscribe(&mut self, topic: &str) -> Result<(), TransportError>;

    async fn publish(&mut self, topic: &str, payload: &[u8]) -> Result<(), TransportError>;
}

/// The real transport, over rumqttc with rustls.
#[derive(Default)]
pub struct RumqttcTransport {
    client: Option<AsyncClient>,
    event_loop: Option<JoinHandle<()>>,
}

impl RumqttcTransport {
    pub fn new() -> Self {
        Self::default()
    }

    fn client(&self) -> Result<&AsyncClient, TransportError> {
        self.client
            .as_ref()
            .ok_or_else(|| TransportError("not connected".to_string()))
    }
}

#[async_trait]
impl Transport for RumqttcTransport {
    async fn connect(
        &mut self,
        credentials: &MqttCredentials,
        client_id: &str,
    ) -> Result<MessageStream, TransportError> {
        if let Some(task) = self.event_loop.take() {
            task.abort();
        }

        let mut options = MqttOptions::new(
            client_id.to_string(),
            credentials.host.clone(),
            credentials.port,
        );
        options.set_credentials(credentials.username.clone(), credentials.password.clone());
        options.set_keep_alive(KEEPALIVE);
        options.set_clean_session(true);
        options.set_transport(rumqttc::Transport::Tls(TlsConfiguration::Rustls(
            crate::tls::client_config().map_err(TransportError)?,
        )));

        let (client, mut event_loop) = AsyncClient::new(options, 32);
        let (tx, rx) = mpsc::unbounded_channel();

        let task = tokio::spawn(async move {
            loop {
                match event_loop.poll().await {
                    Ok(Event::Incoming(Packet::Publish(publish))) => {
                        let message = Message {
                            topic: publish.topic.to_string(),
                            payload: publish.payload.to_vec(),
                        };
                        if tx.send(message).is_err() {
                            break;
                        }
                    }
                    // `subscribe` returns as soon as the packet is queued, so
                    // this is the only place a refused subscription is
                    // visible. Without it, a rejected topic is
                    // indistinguishable from a device that has gone quiet.
                    Ok(Event::Incoming(Packet::SubAck(ack))) => {
                        if ack
                            .return_codes
                            .iter()
                            .any(|code| matches!(code, SubscribeReasonCode::Failure))
                        {
                            tracing::warn!(
                                "EcoFlow broker refused a subscription: {:?}",
                                ack.return_codes
                            );
                        } else {
                            tracing::debug!("EcoFlow subscription accepted");
                        }
                    }
                    Ok(Event::Incoming(Packet::ConnAck(ack))) => {
                        tracing::debug!("EcoFlow broker accepted the connection: {:?}", ack.code);
                    }
                    Ok(event) => tracing::trace!("EcoFlow MQTT event: {event:?}"),
                    Err(e) => {
                        // Ending the loop hands control back to the
                        // integration, which owns the reconnect policy —
                        // including re-authenticating, which this task cannot
                        // do.
                        tracing::debug!("EcoFlow MQTT event loop stopped: {e}");
                        break;
                    }
                }
            }
        });

        self.client = Some(client);
        self.event_loop = Some(task);

        Ok(MessageStream::new(rx))
    }

    async fn subscribe(&mut self, topic: &str) -> Result<(), TransportError> {
        self.client()?
            .subscribe(topic, QOS)
            .await
            .map_err(|e| TransportError(e.to_string()))
    }

    async fn publish(&mut self, topic: &str, payload: &[u8]) -> Result<(), TransportError> {
        self.client()?
            .publish(topic, QOS, false, payload)
            .await
            .map_err(|e| TransportError(e.to_string()))
    }
}

impl Drop for RumqttcTransport {
    fn drop(&mut self) {
        if let Some(task) = self.event_loop.take() {
            task.abort();
        }
    }
}
