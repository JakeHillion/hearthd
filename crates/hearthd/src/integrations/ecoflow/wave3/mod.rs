//! The EcoFlow Wave 3 portable air conditioner.
//!
//! The Wave 3 speaks protobuf on its telemetry, `set` and `set_reply` topics.
//! There is no JSON anywhere in the useful path: payloads are raw protobuf
//! bytes, neither base64-encoded nor length-prefixed nor wrapped.
//!
//! | Module | Contents | Attribution |
//! | --- | --- | --- |
//! | `wire` | envelope, framing constants, obfuscation, dispatch | yes |
//! | `fields` | field numbers for the four payload messages | yes |
//! | `semantics` | mode and preset encodings, ranges, behavioural rules | yes |
//! | `codec` | decoding into Rust, encoding config writes | no |
//! | `state` | sparse-delta merge cache | no |
//! | `matter` | mapping onto hearthd's Matter data model | no |
//!
//! The attributed modules record what the `tolwi/hassio-ecoflow-cloud` Home
//! Assistant custom component (Apache-2.0, commit `a7ebbba`) established about
//! how this device behaves; each names the upstream file it came from. The
//! unattributed modules are hearthd's own engineering. No code was copied from
//! that project.
//!
//! # Requesting a snapshot
//!
//! Devices push state; nothing arrives until they do. The historical way to
//! ask for a snapshot is a JSON `latestQuotas` request on the `get` topic, but
//! protobuf-only firmware ignores it. The Wave 3's native equivalent is a
//! config write setting `active_display_property_full_upload` and
//! `active_runtime_property_full_upload`, which hearthd sends immediately
//! after subscribing.
//!
//! # Telemetry arrives in bursts, not on a cadence
//!
//! The device advertises upload periods — 120 s full and 2 s incremental for
//! display properties, 300 s and 60 s for runtime — but does not keep to them.
//! On a real unit, telemetry arrived in bursts of a minute or two separated by
//! gaps of over an hour, with the bursts coinciding with someone opening the
//! device's page in the EcoFlow app. Throughout one 82-minute silence the MQTT
//! session stayed connected, all five subscriptions were acknowledged, and a
//! config write sent during it was acknowledged by the device.
//!
//! So a Wave 3 that has said nothing for a long time is normal, not broken.
//! Anything above this integration has to tolerate stale readings, and the
//! staleness threshold is set accordingly.
//!
//! # Settled against hardware
//!
//! - **The `active_*_full_upload` snapshot request does not work.** The device
//!   accepts the write — it acknowledges it with `configOk` set — and then
//!   sends nothing. Observed across several sessions, once with no upload for
//!   the following 82 minutes. hearthd still sends it, because it is harmless
//!   and costs one message, but it cannot be relied on.
//! - **Submode 1 means "no preset".** The upstream notes describe 1 as never
//!   observed on the grounds that the app never sends it; the device reports
//!   it whenever no preset is selected. See `semantics::Preset`.
//! - **The upload-period fields are milliseconds, not seconds.**
//!
//! # Unverified
//!
//! Flagged so nobody mistakes inference for fact:
//!
//! - **`cmd_id` 1 versus 21.** Both carry display properties; which is full
//!   and which incremental is inferred. Treating both as deltas is correct
//!   either way.
//! - **`cfgPowerOff`** (config field 3). Present in the schema, unused by the
//!   app for this device. `cfg_sys_pause` is the tested path.
//! - **BMS alarm, protection and fault bitfield layouts.** Bit meanings
//!   unknown, so hearthd does not surface them.
//! - **`check_type`, `d_src`, `d_dest`, `product_id`, `version`,
//!   `payload_ver`.** Constants that work; semantics unknown.
//! - **`pow_get_bms` sign convention.** Assumed positive-in, negative-out.
//! - **Scheduled tasks.** Message shapes known, behaviour untested. hearthd
//!   schedules automations itself, so these are deliberately unimplemented.
//! - **Battery capacity and cell-voltage units.** mAh and mV assumed.

use std::fmt;

pub mod codec;
pub mod fields;
pub mod matter;
pub mod semantics;
pub mod state;
pub mod wire;

/// A failure decoding a Wave 3 frame.
///
/// Every variant means the frame is unusable. Callers log and drop it rather
/// than tearing down the connection: firmware updates add fields and
/// occasionally new command IDs, and a decoder that treats novelty as fatal
/// stops working after a device update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Protobuf(crate::integrations::ecoflow::protobuf::Error),
    /// The outer message carried no header field.
    MissingHeader,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Protobuf(e) => write!(f, "malformed protobuf: {e}"),
            Error::MissingHeader => write!(f, "frame has no header"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Protobuf(e) => Some(e),
            Error::MissingHeader => None,
        }
    }
}

impl From<crate::integrations::ecoflow::protobuf::Error> for Error {
    fn from(e: crate::integrations::ecoflow::protobuf::Error) -> Self {
        Error::Protobuf(e)
    }
}
