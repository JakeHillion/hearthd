//! MQTT client identity and the per-device topic layout.
//!
//! # Client id
//!
//! ```text
//! ANDROID_ + uppercase hex of a random UUIDv4 (32 chars, no dashes) + _ + userId
//! ```
//!
//! for example `ANDROID_3F2C1A9B4D5E6F708192A3B4C5D6E7F8_1234567890`.
//!
//! The broker enforces this shape and rejects a free-form client id as "not
//! authorised". The random component matters for a second reason: EcoFlow
//! drops an existing session when a new one connects with the same client id,
//! so a fresh UUID keeps the daemon from fighting the phone app for the
//! account.
//!
//! Generate the UUID once per process and reuse it across reconnects. Then a
//! reconnect replaces the daemon's own previous session instead of
//! accumulating sessions.
//!
//! # Topics
//!
//! With the account's `userId` and a device serial:
//!
//! | Purpose | Topic | Direction |
//! | --- | --- | --- |
//! | Telemetry push | `/app/device/property/{sn}` | device to client |
//! | Command | `/app/{userId}/{sn}/thing/property/set` | client to device |
//! | Command ack | `/app/{userId}/{sn}/thing/property/set_reply` | device to client |
//! | Snapshot request | `/app/{userId}/{sn}/thing/property/get` | client to device |
//! | Snapshot reply | `/app/{userId}/{sn}/thing/property/get_reply` | device to client |
//!
//! The telemetry topic is deliberately not user-scoped: it is the device's own
//! topic, and it carries the bulk of the state. Traffic the client publishes
//! on `set` and `get` is echoed back on those same topics, so a client sees
//! its own writes.
//!
//! # Attribution
//!
//! The `ANDROID_{UUID}_{userId}` construction, including the observation that
//! other shapes are rejected as unauthorised, and the topic layout were
//! reverse-engineered by the `tolwi/hassio-ecoflow-cloud` Home Assistant
//! custom component (Apache-2.0), read at commit `a7ebbba`, in
//! `custom_components/ecoflow_cloud/api/private_api.py` and
//! `custom_components/ecoflow_cloud/api/__init__.py`.
//!
//! What that project provided is knowledge of the naming scheme. The Rust
//! below is original to hearthd. No code was copied.

use rand::Rng;

/// Build the MQTT client id from a per-process UUID and the account id.
pub fn client_id(uuid_hex: &str, user_id: &str) -> String {
    format!("ANDROID_{uuid_hex}_{user_id}")
}

/// Generate the random component of the client id: a UUIDv4's 32 hex digits,
/// uppercase and without dashes.
///
/// The version and variant bits are set as UUIDv4 requires. Nothing is known
/// to check them, but a value that claims to be a UUIDv4 should be one.
pub fn random_uuid_hex() -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill(&mut bytes);

    bytes[6] = (bytes[6] & 0x0F) | 0x40;
    bytes[8] = (bytes[8] & 0x3F) | 0x80;

    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

/// Telemetry, where the bulk of device state arrives. Not user-scoped.
pub fn telemetry(serial: &str) -> String {
    format!("/app/device/property/{serial}")
}

/// Commands, published by the client.
pub fn set(user_id: &str, serial: &str) -> String {
    format!("/app/{user_id}/{serial}/thing/property/set")
}

/// Command acknowledgements.
pub fn set_reply(user_id: &str, serial: &str) -> String {
    format!("/app/{user_id}/{serial}/thing/property/set_reply")
}

/// Snapshot requests. Retained for completeness: the JSON `latestQuotas`
/// request this topic exists for is ignored by protobuf-only firmware, so the
/// Wave 3 is asked for a snapshot with a config write instead.
pub fn get(user_id: &str, serial: &str) -> String {
    format!("/app/{user_id}/{serial}/thing/property/get")
}

/// Snapshot replies.
pub fn get_reply(user_id: &str, serial: &str) -> String {
    format!("/app/{user_id}/{serial}/thing/property/get_reply")
}

/// Every topic to subscribe to for one device.
pub fn all_for_device(user_id: &str, serial: &str) -> Vec<String> {
    vec![
        telemetry(serial),
        set(user_id, serial),
        set_reply(user_id, serial),
        get(user_id, serial),
        get_reply(user_id, serial),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_id_has_the_shape_the_broker_requires() {
        let id = client_id("3F2C1A9B4D5E6F708192A3B4C5D6E7F8", "1234567890");
        assert_eq!(id, "ANDROID_3F2C1A9B4D5E6F708192A3B4C5D6E7F8_1234567890");
    }

    #[test]
    fn generated_uuid_is_32_uppercase_hex_digits() {
        let hex = random_uuid_hex();
        assert_eq!(hex.len(), 32);
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(!hex.chars().any(|c| c.is_ascii_lowercase()));
        assert!(!hex.contains('-'));
    }

    #[test]
    fn generated_uuid_sets_the_v4_version_and_variant_bits() {
        for _ in 0..32 {
            let hex = random_uuid_hex();
            // Byte 6's high nibble is the version: 4.
            assert_eq!(&hex[12..13], "4", "version nibble in {hex}");
            // Byte 8's high two bits are the variant: 10, so 8, 9, A or B.
            assert!(
                matches!(&hex[16..17], "8" | "9" | "A" | "B"),
                "variant nibble in {hex}"
            );
        }
    }

    #[test]
    fn each_process_gets_a_distinct_uuid() {
        // Identical client ids make EcoFlow drop the other session, so
        // collisions would have the daemon fighting the phone app.
        let a = random_uuid_hex();
        let b = random_uuid_hex();
        assert_ne!(a, b);
    }

    #[test]
    fn telemetry_is_not_user_scoped() {
        assert_eq!(telemetry("AB123"), "/app/device/property/AB123");
    }

    #[test]
    fn command_topics_are_user_scoped() {
        assert_eq!(set("U1", "AB123"), "/app/U1/AB123/thing/property/set");
        assert_eq!(
            set_reply("U1", "AB123"),
            "/app/U1/AB123/thing/property/set_reply"
        );
        assert_eq!(get("U1", "AB123"), "/app/U1/AB123/thing/property/get");
        assert_eq!(
            get_reply("U1", "AB123"),
            "/app/U1/AB123/thing/property/get_reply"
        );
    }

    #[test]
    fn all_five_topics_are_subscribed_per_device() {
        let topics = all_for_device("U1", "AB123");
        assert_eq!(topics.len(), 5);
        assert!(topics.contains(&telemetry("AB123")));
        assert!(topics.contains(&set_reply("U1", "AB123")));
    }
}
