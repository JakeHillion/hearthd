//! The TLS configuration every outbound connection uses.
//!
//! Two deliberate choices here:
//!
//! - **Roots come from `webpki-roots`, not the host.** Relying on the system
//!   trust store makes hearthd fail wherever one is absent, which includes the
//!   nix build sandbox and minimal containers. Carrying the roots keeps every
//!   integration trusting exactly the same set, wherever it runs.
//! - **The provider is passed explicitly rather than installed as the process
//!   default.** No global state, and no dependence on some other component
//!   having installed one first.
//!
//! `ring` rather than `aws-lc-rs`, because the latter needs cmake and bindgen
//! at build time and the flake supplies neither. That is also why every client
//! must come through here: the `*-no-provider` features that keep aws-lc-rs out
//! leave reqwest and rumqttc with no provider of their own, and a client built
//! without one panics on construction.

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
