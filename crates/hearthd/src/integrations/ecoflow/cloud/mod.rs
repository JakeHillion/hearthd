//! The EcoFlow consumer cloud: authentication, topic layout and transport.
//!
//! Device-independent. What to publish on these topics, and how to read what
//! arrives, is the device family's business — see `super::wave3`.
//!
//! `auth` and `topics` carry attribution to `tolwi/hassio-ecoflow-cloud`, which
//! established the handshake and the naming scheme. `transport` and `session`
//! are hearthd's own.

pub mod auth;
pub mod session;
pub mod tls;
pub mod topics;
pub mod transport;
