//! The TLS configuration shared by both connections to EcoFlow.
//!
//! Certificate verification is mandatory and not configurable. The account
//! credentials, the bearer token and the MQTT credentials are all bearer-style
//! secrets — anyone who intercepts them controls every device on the account —
//! so an escape hatch for self-signed certificates would have no legitimate
//! use against EcoFlow's own servers.
//!
//! Two deliberate choices here:
//!
//! - **Roots come from `webpki-roots`, not the host.** Relying on the system
//!   trust store makes hearthd fail wherever one is absent, which includes the
//!   nix build sandbox and minimal containers. Carrying the roots keeps the
//!   HTTP and MQTT paths trusting exactly the same set, wherever it runs.
//! - **The provider is passed explicitly rather than installed as the process
//!   default.** No global state, and no dependence on some other component
//!   having installed one first.
//!
//! `ring` rather than `aws-lc-rs`, because the latter needs cmake and bindgen
//! at build time and the flake supplies neither.

use std::sync::Arc;

/// Build a client configuration trusting the bundled public CA roots.
pub fn client_config() -> Result<Arc<rustls::ClientConfig>, String> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| format!("TLS configuration failed: {e}"))?
    .with_root_certificates(roots)
    .with_no_client_auth();

    Ok(Arc::new(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_configuration_can_be_built_without_a_system_trust_store() {
        // The roots are compiled in, so this holds in a build sandbox as well
        // as on a configured host.
        let config = client_config().expect("TLS configuration should build");
        assert!(!config.crypto_provider().cipher_suites.is_empty());
    }
}
