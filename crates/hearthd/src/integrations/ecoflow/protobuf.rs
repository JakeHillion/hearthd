//! A minimal protobuf wire-format reader and writer.
//!
//! Protobuf's binary encoding is public and stable; this is an implementation
//! of it and carries no attribution. The message *schemas* it is used to
//! decode do carry attribution — see `wave3::wire` and `wave3::fields`.
//!
//! Only what the Wave 3 schema needs is implemented, plus generic skipping so
//! that fields added by a firmware revision do not break decoding.
//!
//! # Wire format
//!
//! Each field is a varint tag followed by a value, where
//! `tag = (field_number << 3) | wire_type`:
//!
//! | Wire type | Rust types | Encoding |
//! | --- | --- | --- |
//! | 0 varint | `u32`, `i32`, `bool` | base-128, 7 bits per byte, continuation bit 0x80 |
//! | 1 fixed64 | unused | 8 bytes |
//! | 2 length-delimited | `String`, bytes, nested messages | varint length, then that many bytes |
//! | 5 fixed32 | `f32` | 4 bytes, IEEE-754 single, little-endian |
//!
//! # Two traps worth naming
//!
//! `i32` is **sign-extended, not zig-zag encoded**. A negative `i32` occupies
//! a full ten bytes. Zig-zag applies to `sint32`/`sint64`, which this schema
//! never uses, so applying it here would silently corrupt every negative
//! value.
//!
//! Repeated scalar fields arrive in either of two shapes: packed (one
//! length-delimited field whose body is a run of varints, the proto3 default)
//! or as several single tags. Readers must accept both.

use std::fmt;

/// Largest number of bytes a valid varint may occupy (64 bits at 7 bits per
/// byte).
const MAX_VARINT_BYTES: usize = 10;

/// Protobuf wire types.
///
/// `Fixed64` is unused by the schemas hearthd decodes but is still recognised
/// so that an unknown field using it can be skipped rather than aborting the
/// parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireType {
    Varint,
    Fixed64,
    Len,
    Fixed32,
}

impl WireType {
    fn from_tag(bits: u32) -> Result<Self, Error> {
        match bits {
            0 => Ok(WireType::Varint),
            1 => Ok(WireType::Fixed64),
            2 => Ok(WireType::Len),
            5 => Ok(WireType::Fixed32),
            other => Err(Error::UnknownWireType(other)),
        }
    }

    fn as_bits(self) -> u32 {
        match self {
            WireType::Varint => 0,
            WireType::Fixed64 => 1,
            WireType::Len => 2,
            WireType::Fixed32 => 5,
        }
    }
}

/// A protobuf decoding failure.
///
/// Every variant means "this buffer is not the message we expected". Callers
/// log and drop the frame rather than tearing down the connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Ran off the end of the buffer mid-value.
    UnexpectedEof,
    /// A varint ran past ten bytes, so it cannot be a valid 64-bit value.
    VarintTooLong,
    /// Wire type 3, 4 (deprecated groups) or 6, 7 (never assigned).
    UnknownWireType(u32),
    /// A length prefix exceeded the bytes actually remaining.
    LengthOutOfRange { len: u64, remaining: usize },
    /// A string field held bytes that are not valid UTF-8.
    InvalidUtf8,
    /// Field number 0, which protobuf reserves.
    ZeroFieldNumber,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::UnexpectedEof => write!(f, "unexpected end of protobuf buffer"),
            Error::VarintTooLong => write!(f, "varint longer than {MAX_VARINT_BYTES} bytes"),
            Error::UnknownWireType(w) => write!(f, "unknown protobuf wire type {w}"),
            Error::LengthOutOfRange { len, remaining } => write!(
                f,
                "length-delimited field claims {len} bytes but only {remaining} remain"
            ),
            Error::InvalidUtf8 => write!(f, "string field is not valid UTF-8"),
            Error::ZeroFieldNumber => write!(f, "field number 0 is reserved"),
        }
    }
}

impl std::error::Error for Error {}

/// Reads protobuf fields out of a byte buffer.
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// True once every byte has been consumed.
    pub fn is_empty(&self) -> bool {
        self.pos >= self.buf.len()
    }

    /// Read the next field's number and wire type, or `None` at end of buffer.
    pub fn read_tag(&mut self) -> Result<Option<(u32, WireType)>, Error> {
        if self.is_empty() {
            return Ok(None);
        }
        let tag = self.read_varint()?;
        let field = (tag >> 3) as u32;
        if field == 0 {
            return Err(Error::ZeroFieldNumber);
        }
        let wire = WireType::from_tag((tag & 0b111) as u32)?;
        Ok(Some((field, wire)))
    }

    pub fn read_varint(&mut self) -> Result<u64, Error> {
        let mut value: u64 = 0;
        for byte_index in 0..MAX_VARINT_BYTES {
            let byte = *self.buf.get(self.pos).ok_or(Error::UnexpectedEof)?;
            self.pos += 1;
            // The final byte of a 10-byte varint contributes only bit 63, so
            // shifting by 63 is the most we ever do and cannot overflow.
            value |= u64::from(byte & 0x7F) << (7 * byte_index);
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(Error::VarintTooLong)
    }

    pub fn read_u32(&mut self) -> Result<u32, Error> {
        Ok(self.read_varint()? as u32)
    }

    /// Read an `int32`. Negative values were sign-extended to 64 bits by the
    /// encoder, so reinterpret rather than zig-zag decode.
    pub fn read_i32(&mut self) -> Result<i32, Error> {
        Ok(self.read_varint()? as i64 as i32)
    }

    pub fn read_bool(&mut self) -> Result<bool, Error> {
        Ok(self.read_varint()? != 0)
    }

    pub fn read_f32(&mut self) -> Result<f32, Error> {
        let end = self.pos.checked_add(4).ok_or(Error::UnexpectedEof)?;
        let bytes = self.buf.get(self.pos..end).ok_or(Error::UnexpectedEof)?;
        let array: [u8; 4] = bytes.try_into().map_err(|_| Error::UnexpectedEof)?;
        self.pos = end;
        Ok(f32::from_le_bytes(array))
    }

    /// Borrow the body of a length-delimited field.
    pub fn read_len_slice(&mut self) -> Result<&'a [u8], Error> {
        let len = self.read_varint()?;
        let remaining = self.buf.len() - self.pos;
        let len_usize =
            usize::try_from(len).map_err(|_| Error::LengthOutOfRange { len, remaining })?;
        if len_usize > remaining {
            return Err(Error::LengthOutOfRange { len, remaining });
        }
        let start = self.pos;
        self.pos += len_usize;
        Ok(&self.buf[start..self.pos])
    }

    pub fn read_string(&mut self) -> Result<String, Error> {
        let bytes = self.read_len_slice()?;
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| Error::InvalidUtf8)
    }

    /// Read a repeated scalar field that may be packed or unpacked.
    ///
    /// `wire` is the wire type from the tag: `Len` means a packed run of
    /// varints, anything else a single value. Decoded values are appended to
    /// `out`, so calling this once per occurrence accumulates both shapes.
    ///
    /// Currently unused: the only repeated scalar fields in the Wave 3 schema
    /// are the error-code and reserved-info lists, and hearthd does not
    /// surface those. It is kept because this reader is an implementation of
    /// the wire format rather than of one schema's subset, and because the
    /// packed-or-unpacked rule is easy to get wrong when someone does need it.
    #[allow(dead_code)]
    pub fn read_packed_or_single_varints(
        &mut self,
        wire: WireType,
        out: &mut Vec<u64>,
    ) -> Result<(), Error> {
        match wire {
            WireType::Len => {
                let body = self.read_len_slice()?;
                let mut inner = Reader::new(body);
                while !inner.is_empty() {
                    out.push(inner.read_varint()?);
                }
                Ok(())
            }
            WireType::Varint => {
                out.push(self.read_varint()?);
                Ok(())
            }
            other => {
                self.skip(other)?;
                Ok(())
            }
        }
    }

    /// Advance past a field of the given wire type without interpreting it.
    ///
    /// Firmware revisions add fields; a decoder that errors on an unrecognised
    /// tag stops working after a device update, so unknown fields must always
    /// be skippable.
    pub fn skip(&mut self, wire: WireType) -> Result<(), Error> {
        match wire {
            WireType::Varint => {
                self.read_varint()?;
            }
            WireType::Fixed64 => {
                let end = self.pos.checked_add(8).ok_or(Error::UnexpectedEof)?;
                if end > self.buf.len() {
                    return Err(Error::UnexpectedEof);
                }
                self.pos = end;
            }
            WireType::Fixed32 => {
                let end = self.pos.checked_add(4).ok_or(Error::UnexpectedEof)?;
                if end > self.buf.len() {
                    return Err(Error::UnexpectedEof);
                }
                self.pos = end;
            }
            WireType::Len => {
                self.read_len_slice()?;
            }
        }
        Ok(())
    }
}

/// Builds a protobuf message.
///
/// Fields are emitted in call order. Protobuf permits any order, but encoders
/// conventionally ascend by field number and the Wave 3 test vectors assume
/// that, so callers should write fields in ascending order.
#[derive(Debug, Default)]
pub struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn into_vec(self) -> Vec<u8> {
        self.buf
    }

    fn write_varint(&mut self, mut value: u64) {
        loop {
            let byte = (value & 0x7F) as u8;
            value >>= 7;
            if value == 0 {
                self.buf.push(byte);
                return;
            }
            self.buf.push(byte | 0x80);
        }
    }

    fn write_tag(&mut self, field: u32, wire: WireType) {
        self.write_varint(u64::from(field) << 3 | u64::from(wire.as_bits()));
    }

    pub fn write_u32(&mut self, field: u32, value: u32) {
        self.write_tag(field, WireType::Varint);
        self.write_varint(u64::from(value));
    }

    /// Write an `int32`, sign-extending negatives to a ten-byte varint as the
    /// format requires.
    pub fn write_i32(&mut self, field: u32, value: i32) {
        self.write_tag(field, WireType::Varint);
        self.write_varint(i64::from(value) as u64);
    }

    pub fn write_bool(&mut self, field: u32, value: bool) {
        self.write_tag(field, WireType::Varint);
        self.write_varint(u64::from(value));
    }

    pub fn write_f32(&mut self, field: u32, value: f32) {
        self.write_tag(field, WireType::Fixed32);
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    pub fn write_bytes(&mut self, field: u32, value: &[u8]) {
        self.write_tag(field, WireType::Len);
        self.write_varint(value.len() as u64);
        self.buf.extend_from_slice(value);
    }

    pub fn write_string(&mut self, field: u32, value: &str) {
        self.write_bytes(field, value.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_round_trips_across_byte_boundaries() {
        for value in [0u64, 1, 127, 128, 300, 16_383, 16_384, u64::MAX] {
            let mut w = Writer::new();
            w.write_varint(value);
            let bytes = w.into_vec();
            let mut r = Reader::new(&bytes);
            assert_eq!(r.read_varint().unwrap(), value, "value {value}");
            assert!(r.is_empty());
        }
    }

    #[test]
    fn varint_128_is_two_bytes() {
        let mut w = Writer::new();
        w.write_varint(128);
        assert_eq!(w.into_vec(), vec![0x80, 0x01]);
    }

    #[test]
    fn negative_i32_is_sign_extended_to_ten_bytes() {
        let mut w = Writer::new();
        w.write_i32(1, -1);
        let bytes = w.into_vec();
        // One tag byte plus a ten-byte varint.
        assert_eq!(bytes.len(), 11);
        assert_eq!(
            &bytes[1..],
            &[0xFF; 9].iter().copied().chain([0x01]).collect::<Vec<_>>()[..]
        );

        let mut r = Reader::new(&bytes);
        let (field, wire) = r.read_tag().unwrap().unwrap();
        assert_eq!(field, 1);
        assert_eq!(wire, WireType::Varint);
        assert_eq!(r.read_i32().unwrap(), -1);
    }

    #[test]
    fn negative_i32_is_not_zig_zag() {
        // Zig-zag would encode -1 as 1, a single byte. Guard against anyone
        // "fixing" the encoder to do that.
        let mut w = Writer::new();
        w.write_i32(1, -1);
        assert_ne!(w.into_vec(), vec![0x08, 0x01]);
    }

    #[test]
    fn i32_round_trips_including_extremes() {
        for value in [0i32, 1, -1, i32::MIN, i32::MAX, -12345] {
            let mut w = Writer::new();
            w.write_i32(3, value);
            let bytes = w.into_vec();
            let mut r = Reader::new(&bytes);
            r.read_tag().unwrap().unwrap();
            assert_eq!(r.read_i32().unwrap(), value, "value {value}");
        }
    }

    #[test]
    fn f32_is_little_endian() {
        let mut w = Writer::new();
        w.write_f32(156, 22.0);
        let bytes = w.into_vec();
        // Tag (156 << 3) | 5 = 1253 = 0xE5 0x09, then 22.0f32 little-endian.
        assert_eq!(bytes, vec![0xE5, 0x09, 0x00, 0x00, 0xB0, 0x41]);

        let mut r = Reader::new(&bytes);
        let (field, wire) = r.read_tag().unwrap().unwrap();
        assert_eq!(field, 156);
        assert_eq!(wire, WireType::Fixed32);
        assert_eq!(r.read_f32().unwrap(), 22.0);
    }

    #[test]
    fn string_and_bytes_round_trip() {
        let mut w = Writer::new();
        w.write_string(23, "Android");
        w.write_bytes(1, &[1, 2, 3]);
        let bytes = w.into_vec();

        let mut r = Reader::new(&bytes);
        r.read_tag().unwrap().unwrap();
        assert_eq!(r.read_string().unwrap(), "Android");
        r.read_tag().unwrap().unwrap();
        assert_eq!(r.read_len_slice().unwrap(), &[1, 2, 3]);
    }

    #[test]
    fn packed_and_unpacked_repeated_varints_both_decode() {
        // Packed: one Len field holding 1, 2, 3.
        let mut packed = Writer::new();
        packed.write_bytes(1, &[0x01, 0x02, 0x03]);
        let packed = packed.into_vec();

        let mut out = Vec::new();
        let mut r = Reader::new(&packed);
        let (_, wire) = r.read_tag().unwrap().unwrap();
        r.read_packed_or_single_varints(wire, &mut out).unwrap();
        assert_eq!(out, vec![1, 2, 3]);

        // Unpacked: three separate varint fields with the same number.
        let mut unpacked = Writer::new();
        unpacked.write_u32(1, 1);
        unpacked.write_u32(1, 2);
        unpacked.write_u32(1, 3);
        let unpacked = unpacked.into_vec();

        let mut out = Vec::new();
        let mut r = Reader::new(&unpacked);
        while let Some((_, wire)) = r.read_tag().unwrap() {
            r.read_packed_or_single_varints(wire, &mut out).unwrap();
        }
        assert_eq!(out, vec![1, 2, 3]);
    }

    #[test]
    fn unknown_fields_of_every_wire_type_are_skippable() {
        let mut w = Writer::new();
        w.write_u32(1, 42);
        w.write_f32(2, 1.5);
        w.write_string(3, "skip me");
        // A fixed64 field, which the Wave 3 schema never uses.
        w.write_tag(4, WireType::Fixed64);
        w.buf.extend_from_slice(&[0u8; 8]);
        w.write_u32(5, 7);
        let bytes = w.into_vec();

        let mut r = Reader::new(&bytes);
        let mut last = None;
        while let Some((field, wire)) = r.read_tag().unwrap() {
            if field == 5 {
                last = Some(r.read_u32().unwrap());
            } else {
                r.skip(wire).unwrap();
            }
        }
        assert_eq!(last, Some(7));
    }

    #[test]
    fn truncated_buffers_error_rather_than_panic() {
        // Length prefix claims more than the buffer holds.
        let mut r = Reader::new(&[0x0A, 0x10, 0x01]);
        r.read_tag().unwrap().unwrap();
        assert!(matches!(
            r.read_len_slice(),
            Err(Error::LengthOutOfRange { .. })
        ));

        // Varint continuation bit set at end of buffer.
        let mut r = Reader::new(&[0x80]);
        assert_eq!(r.read_varint(), Err(Error::UnexpectedEof));

        // Fixed32 with only three bytes left.
        let mut r = Reader::new(&[0x00, 0x00, 0x00]);
        assert_eq!(r.read_f32(), Err(Error::UnexpectedEof));
    }

    #[test]
    fn varint_longer_than_ten_bytes_is_rejected() {
        let mut r = Reader::new(&[0xFF; 12]);
        assert_eq!(r.read_varint(), Err(Error::VarintTooLong));
    }

    #[test]
    fn field_number_zero_is_rejected() {
        let mut r = Reader::new(&[0x00]);
        assert_eq!(r.read_tag(), Err(Error::ZeroFieldNumber));
    }

    #[test]
    fn deprecated_group_wire_types_are_rejected() {
        // Field 1, wire type 3 (start group).
        let mut r = Reader::new(&[0x0B]);
        assert_eq!(r.read_tag(), Err(Error::UnknownWireType(3)));
    }

    #[test]
    fn invalid_utf8_string_is_rejected() {
        let mut w = Writer::new();
        w.write_bytes(1, &[0xFF, 0xFE]);
        let bytes = w.into_vec();
        let mut r = Reader::new(&bytes);
        r.read_tag().unwrap().unwrap();
        assert_eq!(r.read_string(), Err(Error::InvalidUtf8));
    }
}
