//! EcoFlow Wave 3 frame envelope: header layout, framing constants, payload
//! obfuscation and message dispatch.
//!
//! # The envelope
//!
//! Every message in both directions is the same two-level structure. The outer
//! message has a single field 1 holding the header; the header's field 1
//! (`pdata`) holds a second, independently serialised protobuf message whose
//! type is determined by `(cmd_func, cmd_id)`.
//!
//! ```text
//! Wave3Message { 1: Wave3Header header }
//! ```
//!
//! Header fields. Only field numbers and wire types travel, so the names are
//! labels for humans. Field 13 does not exist in the schema; fields not listed
//! are unused by the Wave 3.
//!
//! | # | Name | Type | Notes |
//! | --- | --- | --- | --- |
//! | 1 | `pdata` | bytes | the nested payload message |
//! | 2 | `src` | int32 | 32 means "the app" |
//! | 3 | `dest` | int32 | 66 means the Wave 3 |
//! | 4 | `d_src` | int32 | |
//! | 5 | `d_dest` | int32 | |
//! | 6 | `enc_type` | int32 | see "Payload obfuscation" below |
//! | 7 | `check_type` | int32 | a constant, not a checksum |
//! | 8 | `cmd_func` | int32 | dispatch, with `cmd_id` |
//! | 9 | `cmd_id` | int32 | dispatch, with `cmd_func` |
//! | 10 | `data_len` | int32 | must equal `pdata` length in bytes |
//! | 11 | `need_ack` | int32 | 1 asks the device to reply |
//! | 12 | `is_ack` | int32 | |
//! | 14 | `seq` | int32 | also the obfuscation key, see below |
//! | 15 | `product_id` | int32 | |
//! | 16 | `version` | int32 | |
//! | 17 | `payload_ver` | int32 | |
//! | 18 | `time_snap` | int32 | |
//! | 19 | `is_rw_cmd` | int32 | |
//! | 20 | `is_queue` | int32 | |
//! | 21 | `ack_type` | int32 | |
//! | 22 | `code` | string | |
//! | 23 | `from` | string | literally named `from` in EcoFlow's schema |
//! | 24 | `module_sn` | string | |
//! | 25 | `device_sn` | string | |
//!
//! # Field numbers are per-message
//!
//! Field numbers mean something only within one message type, and EcoFlow
//! reuses them freely. Field 209 is `plug_in_info_ac_in_chg_pow_max` in a
//! display upload but `cfg_power_off_delay_set` in a config write; field 4 is
//! `pow_out_sum_w` in the former and `cfg_main_power` in the latter. Decode
//! tables must therefore key on (message type, field number), never on the
//! field number alone. The display and runtime uploads happen to use disjoint
//! numbers, but that is EcoFlow's convention rather than a guarantee.
//!
//! # Payload obfuscation
//!
//! `enc_type == 1` does not by itself mean the payload is scrambled — the rule
//! is directional:
//!
//! - **Outbound** (client to device, `src == 32`): `pdata` is written in the
//!   clear even though `enc_type` is 1.
//! - **Inbound** (device to client): when `enc_type == 1` *and* `src != 32`,
//!   every byte of `pdata` is XORed with `seq & 0xFF`.
//!
//! A key of 0 makes this a no-op, which is legitimate and happens naturally
//! for some sequence numbers. This is obfuscation, not encryption: there is no
//! integrity check to verify, and `check_type` is a constant rather than a
//! checksum.
//!
//! # Attribution
//!
//! EcoFlow does not document this protocol. The envelope structure, the header
//! field numbers and types, the outbound framing constants, the
//! `(cmd_func, cmd_id)` dispatch table and the obfuscation rule including its
//! `src != 32` condition were reverse-engineered by the
//! `tolwi/hassio-ecoflow-cloud` Home Assistant custom component (Apache-2.0),
//! read at commit `a7ebbba`, in
//! `custom_components/ecoflow_cloud/devices/internal/proto/wave3.proto` and
//! `custom_components/ecoflow_cloud/devices/internal/wave3.py`. Message names
//! have been shortened here; field numbers and wire types are unchanged
//! because they are what the device requires.
//!
//! What that project provided is knowledge of the wire format. The Rust below,
//! its error handling and its tests are original to hearthd. No code was
//! copied.

use super::Error;
use crate::integrations::ecoflow::protobuf::Reader;
use crate::integrations::ecoflow::protobuf::WireType;
use crate::integrations::ecoflow::protobuf::Writer;

/// Field number of the header inside the outer `Wave3Message`.
const FIELD_OUTER_HEADER: u32 = 1;

// Header field numbers used by this implementation.
const FIELD_PDATA: u32 = 1;
const FIELD_SRC: u32 = 2;
const FIELD_DEST: u32 = 3;
const FIELD_D_SRC: u32 = 4;
const FIELD_D_DEST: u32 = 5;
const FIELD_ENC_TYPE: u32 = 6;
const FIELD_CHECK_TYPE: u32 = 7;
const FIELD_CMD_FUNC: u32 = 8;
const FIELD_CMD_ID: u32 = 9;
const FIELD_DATA_LEN: u32 = 10;
const FIELD_NEED_ACK: u32 = 11;
const FIELD_SEQ: u32 = 14;
const FIELD_VERSION: u32 = 16;
const FIELD_PAYLOAD_VER: u32 = 17;
const FIELD_IS_RW_CMD: u32 = 19;
const FIELD_FROM: u32 = 23;
const FIELD_DEVICE_SN: u32 = 25;

/// `src` value identifying the app, and the value the device echoes back as
/// something *other* than itself. The obfuscation rule keys off this.
pub const SRC_APP: i32 = 32;

/// `dest` value identifying the Wave 3.
const DEST_WAVE3: i32 = 66;

const D_SRC: i32 = 1;
const D_DEST: i32 = 1;

/// `enc_type` the app always sends. Note this does not mean outbound payloads
/// are obfuscated; see the module docs.
const ENC_TYPE_OBFUSCATED: i32 = 1;

const CHECK_TYPE: i32 = 3;
const VERSION: i32 = 3;
const PAYLOAD_VER: i32 = 1;
const IS_RW_CMD: i32 = 1;
const NEED_ACK: i32 = 1;

/// The firmware accepts commands claiming to come from the Android app.
const FROM_ANDROID: &str = "Android";

/// `cmd_func` every Wave 3 frame uses.
pub const CMD_FUNC: i32 = 254;

/// `cmd_id` for a full display-property upload (device to client).
pub const CMD_ID_DISPLAY_FULL: i32 = 1;
/// `cmd_id` for an incremental display-property upload (device to client).
pub const CMD_ID_DISPLAY_INCREMENTAL: i32 = 21;
/// `cmd_id` for a runtime-property upload (device to client).
pub const CMD_ID_RUNTIME: i32 = 22;
/// `cmd_id` for a config write (client to device).
pub const CMD_ID_CONFIG_WRITE: i32 = 17;
/// `cmd_id` for a config-write acknowledgement (device to client).
pub const CMD_ID_CONFIG_WRITE_ACK: i32 = 18;

/// Which message `pdata` holds, resolved from `(cmd_func, cmd_id)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Payload {
    /// Display properties. `cmd_id` 1 and 21 both land here.
    ///
    /// 1 appears to be the full upload and 21 the incremental one, but that is
    /// inferred rather than confirmed. It does not matter: both are sparse
    /// deltas and merge identically, so treating them the same is correct
    /// either way.
    DisplayPropertyUpload,
    RuntimePropertyUpload,
    ConfigWriteAck,
}

/// Resolve a frame's payload type.
///
/// Returns `None` for anything not in the table. Unknown frames are logged and
/// dropped rather than guessed at — falling back to parsing them as a display
/// upload, as the upstream integration does, will happily decode garbage.
pub fn dispatch(cmd_func: i32, cmd_id: i32) -> Option<Payload> {
    if cmd_func != CMD_FUNC {
        return None;
    }
    match cmd_id {
        CMD_ID_DISPLAY_FULL | CMD_ID_DISPLAY_INCREMENTAL => Some(Payload::DisplayPropertyUpload),
        CMD_ID_RUNTIME => Some(Payload::RuntimePropertyUpload),
        CMD_ID_CONFIG_WRITE_ACK => Some(Payload::ConfigWriteAck),
        _ => None,
    }
}

/// A decoded inbound frame with its payload already de-obfuscated.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub payload: Payload,
    pub pdata: Vec<u8>,
    pub device_sn: Option<String>,
    pub seq: u32,
}

/// Header fields recovered while parsing, before dispatch.
#[derive(Debug, Default)]
struct RawHeader {
    pdata: Vec<u8>,
    src: i32,
    seq: u32,
    enc_type: i32,
    cmd_func: i32,
    cmd_id: i32,
    device_sn: Option<String>,
}

/// Apply the XOR obfuscation in place. Self-inverse, so this both applies and
/// removes it.
fn deobfuscate(pdata: &mut [u8], seq: u32) {
    let key = (seq & 0xFF) as u8;
    if key == 0 {
        return;
    }
    for byte in pdata.iter_mut() {
        *byte ^= key;
    }
}

/// Decode an inbound frame.
///
/// Returns `Ok(None)` for a frame that is well-formed but carries nothing to
/// merge — an empty `pdata`, or a `(cmd_func, cmd_id)` outside the dispatch
/// table. Those are normal: firmware revisions introduce new message types.
pub fn decode_frame(bytes: &[u8]) -> Result<Option<Frame>, Error> {
    let header_bytes = read_outer(bytes)?;
    let header = read_header(header_bytes)?;

    if header.pdata.is_empty() {
        return Ok(None);
    }

    let payload = match dispatch(header.cmd_func, header.cmd_id) {
        Some(p) => p,
        None => return Ok(None),
    };

    let mut pdata = header.pdata;
    if header.enc_type == ENC_TYPE_OBFUSCATED && header.src != SRC_APP {
        deobfuscate(&mut pdata, header.seq);
    }

    Ok(Some(Frame {
        payload,
        pdata,
        device_sn: header.device_sn,
        seq: header.seq,
    }))
}

/// Unwrap the outer message and return the header's bytes.
fn read_outer(bytes: &[u8]) -> Result<&[u8], Error> {
    let mut reader = Reader::new(bytes);
    while let Some((field, wire)) = reader.read_tag()? {
        if field == FIELD_OUTER_HEADER && wire == WireType::Len {
            return reader.read_len_slice().map_err(Error::from);
        }
        reader.skip(wire)?;
    }
    Err(Error::MissingHeader)
}

fn read_header(bytes: &[u8]) -> Result<RawHeader, Error> {
    let mut header = RawHeader::default();
    let mut reader = Reader::new(bytes);

    while let Some((field, wire)) = reader.read_tag()? {
        match (field, wire) {
            (FIELD_PDATA, WireType::Len) => header.pdata = reader.read_len_slice()?.to_vec(),
            (FIELD_SRC, WireType::Varint) => header.src = reader.read_i32()?,
            (FIELD_ENC_TYPE, WireType::Varint) => header.enc_type = reader.read_i32()?,
            (FIELD_CMD_FUNC, WireType::Varint) => header.cmd_func = reader.read_i32()?,
            (FIELD_CMD_ID, WireType::Varint) => header.cmd_id = reader.read_i32()?,
            (FIELD_SEQ, WireType::Varint) => header.seq = reader.read_varint()? as u32,
            (FIELD_DEVICE_SN, WireType::Len) => header.device_sn = Some(reader.read_string()?),
            _ => reader.skip(wire)?,
        }
    }

    Ok(header)
}

/// Frame a `ConfigWrite` payload for publication.
///
/// `seq` must be present on every command: besides ordering, it is the
/// obfuscation key the device will use for frames it sends back. A fresh
/// random value per command is what the app does and what the firmware
/// accepts; it is deliberately not a monotonic counter.
pub fn encode_config_write(pdata: &[u8], device_sn: &str, seq: u32) -> Vec<u8> {
    let mut header = Writer::new();

    // Ascending field order: protobuf permits any order, but encoders
    // conventionally ascend and the test vectors assume it.
    header.write_bytes(FIELD_PDATA, pdata);
    header.write_i32(FIELD_SRC, SRC_APP);
    header.write_i32(FIELD_DEST, DEST_WAVE3);
    header.write_i32(FIELD_D_SRC, D_SRC);
    header.write_i32(FIELD_D_DEST, D_DEST);
    header.write_i32(FIELD_ENC_TYPE, ENC_TYPE_OBFUSCATED);
    header.write_i32(FIELD_CHECK_TYPE, CHECK_TYPE);
    header.write_i32(FIELD_CMD_FUNC, CMD_FUNC);
    header.write_i32(FIELD_CMD_ID, CMD_ID_CONFIG_WRITE);
    header.write_i32(FIELD_DATA_LEN, pdata.len() as i32);
    header.write_i32(FIELD_NEED_ACK, NEED_ACK);
    header.write_u32(FIELD_SEQ, seq);
    header.write_i32(FIELD_VERSION, VERSION);
    header.write_i32(FIELD_PAYLOAD_VER, PAYLOAD_VER);
    header.write_i32(FIELD_IS_RW_CMD, IS_RW_CMD);
    header.write_string(FIELD_FROM, FROM_ANDROID);
    header.write_string(FIELD_DEVICE_SN, device_sn);

    let mut outer = Writer::new();
    outer.write_bytes(FIELD_OUTER_HEADER, &header.into_vec());
    outer.into_vec()
}

/// Build an inbound frame the way the device would, for tests elsewhere in
/// this crate.
///
/// `pdata` is given in the clear and obfuscated here if `src` and `enc_type`
/// call for it, so callers state the plaintext they expect to get back.
#[cfg(test)]
pub fn encode_inbound_for_test(
    cmd_id: i32,
    pdata: &[u8],
    seq: u32,
    src: i32,
    enc_type: i32,
    device_sn: &str,
) -> Vec<u8> {
    let mut scrambled = pdata.to_vec();
    if enc_type == ENC_TYPE_OBFUSCATED && src != SRC_APP {
        deobfuscate(&mut scrambled, seq);
    }

    let mut header = Writer::new();
    header.write_bytes(FIELD_PDATA, &scrambled);
    header.write_i32(FIELD_SRC, src);
    header.write_i32(FIELD_ENC_TYPE, enc_type);
    header.write_i32(FIELD_CMD_FUNC, CMD_FUNC);
    header.write_i32(FIELD_CMD_ID, cmd_id);
    header.write_u32(FIELD_SEQ, seq);
    header.write_string(FIELD_DEVICE_SN, device_sn);

    let mut outer = Writer::new();
    outer.write_bytes(FIELD_OUTER_HEADER, &header.into_vec());
    outer.into_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::ecoflow::wave3::fields::config_write;

    /// The reference vector: set the target temperature to 22.0 C on device
    /// `AB123` with `seq = 500`.
    ///
    /// `pdata` is `ConfigWrite { cfg_temp_set = 22.0 }` — field 156, float:
    ///
    /// ```text
    /// E5 09          tag: (156 << 3) | 5 = 1253, as a varint
    /// 00 00 B0 41    22.0f32, little-endian
    /// ```
    ///
    /// giving `data_len = 6`, a 59-byte header and a 61-byte frame.
    fn reference_frame() -> Vec<u8> {
        #[rustfmt::skip]
        let bytes = vec![
            0x0A, 0x3B,                                     // outer field 1, len 59
            0x0A, 0x06, 0xE5, 0x09, 0x00, 0x00, 0xB0, 0x41, //  1 pdata (len 6)
            0x10, 0x20,                                     //  2 src         = 32
            0x18, 0x42,                                     //  3 dest        = 66
            0x20, 0x01,                                     //  4 d_src       = 1
            0x28, 0x01,                                     //  5 d_dest      = 1
            0x30, 0x01,                                     //  6 enc_type    = 1
            0x38, 0x03,                                     //  7 check_type  = 3
            0x40, 0xFE, 0x01,                               //  8 cmd_func    = 254
            0x48, 0x11,                                     //  9 cmd_id      = 17
            0x50, 0x06,                                     // 10 data_len    = 6
            0x58, 0x01,                                     // 11 need_ack    = 1
            0x70, 0xF4, 0x03,                               // 14 seq         = 500
            0x80, 0x01, 0x03,                               // 16 version     = 3
            0x88, 0x01, 0x01,                               // 17 payload_ver = 1
            0x98, 0x01, 0x01,                               // 19 is_rw_cmd   = 1
            0xBA, 0x01, 0x07, 0x41, 0x6E, 0x64, 0x72, 0x6F, 0x69, 0x64, // 23 from = "Android"
            0xCA, 0x01, 0x05, 0x41, 0x42, 0x31, 0x32, 0x33, // 25 device_sn = "AB123"
        ];
        bytes
    }

    #[test]
    fn encodes_the_reference_frame_byte_for_byte() {
        let mut pdata = Writer::new();
        pdata.write_f32(config_write::CFG_TEMP_SET, 22.0);
        let pdata = pdata.into_vec();
        assert_eq!(pdata, vec![0xE5, 0x09, 0x00, 0x00, 0xB0, 0x41]);

        let frame = encode_config_write(&pdata, "AB123", 500);
        assert_eq!(frame.len(), 61);
        assert_eq!(frame, reference_frame());
    }

    #[test]
    fn outbound_pdata_is_written_in_the_clear() {
        // enc_type is 1 but src is 32, so the payload must not be scrambled:
        // the plaintext float bytes appear verbatim in the frame.
        let mut pdata = Writer::new();
        pdata.write_f32(config_write::CFG_TEMP_SET, 22.0);
        let frame = encode_config_write(&pdata.into_vec(), "AB123", 500);
        assert!(
            frame
                .windows(6)
                .any(|w| w == [0xE5, 0x09, 0x00, 0x00, 0xB0, 0x41])
        );
    }

    fn inbound_frame(cmd_id: i32, pdata: &[u8], seq: u32, src: i32, enc_type: i32) -> Vec<u8> {
        encode_inbound_for_test(cmd_id, pdata, seq, src, enc_type, "AB123")
    }

    #[test]
    fn inbound_payload_is_deobfuscated() {
        let plain = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let frame = inbound_frame(CMD_ID_DISPLAY_FULL, &plain, 500, 66, 1);
        let decoded = decode_frame(&frame).unwrap().unwrap();
        assert_eq!(decoded.payload, Payload::DisplayPropertyUpload);
        assert_eq!(decoded.pdata, plain);
        assert_eq!(decoded.device_sn.as_deref(), Some("AB123"));
    }

    #[test]
    fn obfuscation_key_zero_is_a_legitimate_no_op() {
        // seq = 256 gives seq & 0xFF == 0.
        let plain = vec![0x01, 0x02, 0x03];
        let frame = inbound_frame(CMD_ID_DISPLAY_FULL, &plain, 256, 66, 1);
        let decoded = decode_frame(&frame).unwrap().unwrap();
        assert_eq!(decoded.pdata, plain);
    }

    #[test]
    fn obfuscation_is_skipped_when_src_is_the_app() {
        // Our own published frames are echoed back on the set topic. They have
        // src == 32 and must not be de-obfuscated even though enc_type is 1.
        let plain = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let frame = inbound_frame(CMD_ID_CONFIG_WRITE_ACK, &plain, 500, SRC_APP, 1);
        let decoded = decode_frame(&frame).unwrap().unwrap();
        assert_eq!(decoded.pdata, plain);
    }

    #[test]
    fn obfuscation_is_skipped_when_enc_type_is_not_one() {
        let plain = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let frame = inbound_frame(CMD_ID_DISPLAY_FULL, &plain, 500, 66, 0);
        let decoded = decode_frame(&frame).unwrap().unwrap();
        assert_eq!(decoded.pdata, plain);
    }

    #[test]
    fn dispatch_table() {
        assert_eq!(dispatch(254, 1), Some(Payload::DisplayPropertyUpload));
        assert_eq!(dispatch(254, 21), Some(Payload::DisplayPropertyUpload));
        assert_eq!(dispatch(254, 22), Some(Payload::RuntimePropertyUpload));
        assert_eq!(dispatch(254, 18), Some(Payload::ConfigWriteAck));
        // Config writes are ours; we never receive one to decode.
        assert_eq!(dispatch(254, 17), None);
        assert_eq!(dispatch(254, 99), None);
        assert_eq!(dispatch(1, 1), None);
    }

    #[test]
    fn unknown_command_ids_are_dropped_not_guessed() {
        let frame = inbound_frame(99, &[0x01, 0x02], 500, 66, 1);
        assert_eq!(decode_frame(&frame).unwrap(), None);
    }

    #[test]
    fn header_only_frames_are_dropped() {
        let frame = inbound_frame(CMD_ID_DISPLAY_FULL, &[], 500, 66, 1);
        assert_eq!(decode_frame(&frame).unwrap(), None);
    }

    #[test]
    fn frames_without_a_header_are_an_error() {
        // An outer message carrying only some other field.
        let mut outer = Writer::new();
        outer.write_u32(7, 1);
        assert!(matches!(
            decode_frame(&outer.into_vec()),
            Err(Error::MissingHeader)
        ));
    }

    #[test]
    fn unknown_header_fields_are_skipped() {
        // A firmware revision adding fields must not break decoding.
        let mut header = Writer::new();
        header.write_bytes(FIELD_PDATA, &[0xAA]);
        header.write_i32(FIELD_SRC, 66);
        header.write_i32(FIELD_ENC_TYPE, 0);
        header.write_i32(FIELD_CMD_FUNC, CMD_FUNC);
        header.write_i32(FIELD_CMD_ID, CMD_ID_RUNTIME);
        header.write_u32(FIELD_SEQ, 7);
        // Fields the schema does not define, in all three wire types.
        header.write_u32(200, 1);
        header.write_f32(201, 2.0);
        header.write_string(202, "surprise");

        let mut outer = Writer::new();
        outer.write_bytes(FIELD_OUTER_HEADER, &header.into_vec());

        let decoded = decode_frame(&outer.into_vec()).unwrap().unwrap();
        assert_eq!(decoded.payload, Payload::RuntimePropertyUpload);
        assert_eq!(decoded.pdata, vec![0xAA]);
    }

    #[test]
    fn deobfuscate_is_self_inverse() {
        let original = vec![0x00, 0x01, 0x7F, 0x80, 0xFF];
        let mut buf = original.clone();
        deobfuscate(&mut buf, 500);
        assert_ne!(buf, original);
        deobfuscate(&mut buf, 500);
        assert_eq!(buf, original);
    }
}
