# Anker SOLIX S2000 / AS220 local BLE protocol facts

Independent factual research handoff, 2026-11-10. The live client credential is redacted and unit serial/MAC are pseudonymized for repository publication; identity examples use the synthetic replacements. This document contains independently written descriptions of wire facts and current-device observations. It contains no upstream source, pseudocode, implementation architecture, or upstream test logic. Numbers and byte strings are protocol constants or explicitly identified factual/synthetic wire examples. This is a specification for an independent implementation, not a claim that every firmware variation has been tested.

## Evidence and confidence

The strongest device evidence is the user's day-zero report dated 2026-11-09 and the user's subsequent corrections. Those corrections take precedence: buy is mode 0 with a 1,100 W charge cap; sell is mode 2 with backup 10%; hold is mode 2 with backup equal to current SoC, changing within 0–2 seconds without drift or recharge. A backup target above current SoC charges. Reaching the backup floor naturally selects pass-through. A zero-watt charge setting still charges at approximately 64 W. Battery-flow byte `a5+01` is reliable; total output `a6+00` agrees with an external plug within 0.5%. Approximately 1% of handshakes fail, recovering on the next attempt.

Evidence labels below:

* **Observed**: reported on the user's AS220, including the corrections above.
* **Reference**: wire behaviour evident in the pinned AS220 client; it worked on the user's unit, but some response bodies were not checked by that client.
* **Module evidence**: independently reported analysis of the same ESP32-C3 communications family, useful for status and authorization semantics; AS220-specific exceptions remain possible.
* **Uncertain**: inference or behaviour not directly confirmed on AS220.

Provenance, for audit by the researcher. An implementer need not and should not open the upstream implementation:

1. User-provided report: the private `s2000-day-zero.md` attachment (not redistributed), especially sections 0, 1.5, 1.6, 5.6, and 10. The subsequent user corrections are recorded above.
2. [AS220 wire exchange reference](https://github.com/flip-dots/SolixBLE/blob/7a1780b287c20846371318773e130a0001e98053/SolixBLE/devices/as220.py).
3. [Cryptographic constants and session cipher reference](https://github.com/flip-dots/SolixBLE/blob/7a1780b287c20846371318773e130a0001e98053/SolixBLE/prime_device.py).
4. [Outer framing and transport reference](https://github.com/flip-dots/SolixBLE/blob/7a1780b287c20846371318773e130a0001e98053/SolixBLE/device.py), [GATT UUID reference](https://github.com/flip-dots/SolixBLE/blob/7a1780b287c20846371318773e130a0001e98053/SolixBLE/const.py).
5. [Captured wire data provenance](https://github.com/flip-dots/SolixBLE/blob/7a1780b287c20846371318773e130a0001e98053/tests/test_as220.py). Only factual device bytes and their field boundaries informed this document; upstream test code and organization are not part of the handoff.
6. [PR65 discussion](https://github.com/flip-dots/SolixBLE/pull/65). Its single-owner claim and some power-field interpretations are superseded by the user's device observations.
7. Same-module supplementary factual analysis, pinned at `705aae0b0195b6563081a53e3a7b3a7dde7e4f3f`: [authorization](https://github.com/flip-dots/SolixBLE/blob/705aae0b0195b6563081a53e3a7b3a7dde7e4f3f/docs/deep-documentation/authorization.md), [negotiation](https://github.com/flip-dots/SolixBLE/blob/705aae0b0195b6563081a53e3a7b3a7dde7e4f3f/docs/deep-documentation/negotiation.md), [framing](https://github.com/flip-dots/SolixBLE/blob/705aae0b0195b6563081a53e3a7b3a7dde7e4f3f/docs/deep-documentation/framing.md), [discovery/GATT](https://github.com/flip-dots/SolixBLE/blob/705aae0b0195b6563081a53e3a7b3a7dde7e4f3f/docs/deep-documentation/discovery-and-gatt.md). Much of this analysis was established on A1783 and A2345, not directly on AS220. It resolves distinctions the AS220 reference client does not enforce.
8. [AS220 field and command map](https://github.com/thomluther/anker-solix-api/blob/646e2b7b/src/anker_solix_api/mqttmap.py) and [command data types](https://github.com/thomluther/anker-solix-api/blob/646e2b7b/src/anker_solix_api/mqttcmdmap.py). These are secondary evidence for untested controls; this document covers BLE only.

## Scope and identity

This specification covers local BLE telemetry, client enrollment/reuse, usage mode, backup percentage, AC charge power, and the additional confirmed local settings listed below. No Wi-Fi provisioning, cloud service, account login, MQTT transport, schedules, factory functions, firmware update, or plaintext downgrade is required or specified.

The current unit is model `AS220`, name `SOLIX S2000`, Bluetooth address `00:11:22:33:44:55`, serial `TEST1234567890123`. Its already enrolled 36-byte ASCII client UUID is `<existing-client-uuid>` (redacted; retain the value from the private secrets configuration). Reuse that exact UUID for this unit; generating a new one changes the client identity and needs a physical confirmation. Do not treat these device-specific values as universal protocol constants.

The family advertises service identifier `0000ff09-0000-1000-8000-00805f9b34fb`. It serves a different GATT service, `8c850001-0302-41c5-b46e-cf057c562025`. Commands use characteristic `8c850002-0302-41c5-b46e-cf057c562025`; notifications use `8c850003-0302-41c5-b46e-cf057c562025`. Enable notifications before the first command. Discover by UUID; ATT handles are not stable identifiers. No OS-level BLE pairing/bonding is part of the local application protocol.

Only one BLE connection can be active. The unit stops advertising while the phone or another client is connected. Absence from a scan is therefore not evidence of unsupported firmware.

Pre-binding identification has limits. The advertised name, family service and configured MAC narrow the candidate; none independently proves AS220 model or serial. A manufacturer record under company ID `ffff` in this module family contains a one-byte format version, six MAC bytes, binding state, two model-code bytes, a SKU substring, and optionally a capability byte. Version 1 generally has a 3-character SKU; version 2 has four. Capability bit `04` advertises encrypted negotiation. The AS220-specific manufacturer model code is **not established** by the available evidence; do not borrow an A1783 code or identify every FF09 device as AS220. Do not rely on the MAC OUI as model identification.

Before sending the registration command, obtain and compare the serial from the authenticated static-key `4829` device-info response. That response contains chip/library/serial/MAC, but no proven AS220 model field. The model can be conclusively checked in authorized telemetry's `a2` record. For an unbound unit this model confirmation may be available only after the button press. Validate the serial against the intended unit before enrollment, and validate both serial and `AS220` in telemetry before enabling controls. Static-key identity is a protocol identification check, not manufacturer-certified identity.

## Outer frame and transport

Hex command IDs in this document are byte pairs in wire order: `4101` means bytes `41 01`. Lengths and multibyte numeric payload values are little-endian; the command byte pair is not to be reversed.

| Frame byte offset | Width | Meaning |
|---|---:|---|
| 0 | 2 | Magic `ff 09` |
| 2 | 2 | Total frame length, unsigned LE16, including magic and final checksum |
| 4 | 3 | Pattern |
| 7 | 2 | Command |
| 9 | variable | Payload |
| last | 1 | XOR of every preceding frame byte |

The minimum outer frame is 10 bytes. Validate the complete length and XOR before using the payload. Each application fragment is separately framed, length-checked and checksummed. ATT operation boundaries identify frames: a ciphertext occurrence of `ff09` is not a framing delimiter. There is no known sequence number or request identifier in the outer header.

Relevant patterns:

| Pattern | Meaning |
|---|---|
| `030001` | Client negotiation messages and ordinary module negotiation replies |
| `030101` | Device-initiated negotiation event, including the physical-button authorization grant |
| `03000f` | Client session command, and some module-generated session error replies |
| `03010f` | Controller replies and pushed session data |
| `030111` | Another observed receive pattern for session data |

The AS220 reference sends negotiation through `4023` on `030001` and ordinary session commands on `03000f`. Dispatch negotiation grants by both command and channel, including `030101`; missing that pattern loses an important authorization event. PR65 additionally reports grant-like `4827` frames on `03010f`/`030111`; accept a valid, authenticated grant on these known patterns when an enrollment is pending, without restarting registration.

The high command nibble carries flags: `40` encryption and `80` application fragmentation. The low 12 command bits carry the message type. A reply sets bit `0800`: e.g. request `4101`, reply `4901`; status request `4100`, ordinary unfragmented reply `4900`, fragmented reply `c900`. Push type `0421` appears encrypted/fragmented as `c421`. Match message type with its applicable flags rather than assuming only one literal flagged encoding exists.

Write without ATT response when supported. The family characteristic offers both write forms; app captures use Write Command. The pinned AS220 reference leaves this choice to its BLE library, and worked directly on the user's host. Write-with-response can fail through proxies in same-family reports. All current required outgoing messages are short; no long configuration upload is necessary.

## Cryptography

All described encrypted payloads use AES-128-GCM with a 16-byte authentication tag appended immediately after ciphertext. The outer header and checksum are outside the GCM payload. The AAD is exactly 16 bytes:

`33 22 11 00 77 66 55 44 bb aa 99 88 ff ee dd cc`.

Before the ECDH reply has been accepted, the static negotiation key is:

`b8 ff 74 22 95 5d 4e b6 d5 54 a2 c4 70 28 05 59`.

Its 12-byte nonce is:

`6b a3 e3 f2 f3 a6 0f 29 71 ce 5d 1f`.

Generate a fresh client P-256/secp256r1 keypair per connection/negotiation attempt. Public-key payloads contain exactly 64 bytes: 32-byte big-endian X followed by 32-byte big-endian Y, without the uncompressed point prefix `04`. The device's point must have the correct width and be a valid, non-infinite point on P-256. ECDH produces a fixed-width, 32-byte big-endian shared X coordinate, including leading zeros. There is no hash, HKDF, salt, or ASCII conversion. Its first 16 bytes are the session AES key; bytes 16 through 27 inclusive are the session 12-byte nonce. Its final four bytes are unused by GCM.

The session nonce remains the same throughout that connection, as required by the observed protocol. There is no per-message nonce transmitted. Reconnection requires a fresh key exchange. Never log the shared secret/session key or retain one across links.

The static key applies through the `4821` device-public-key reply. Starting with `4022`, messages and replies use the session key/nonce, including `4822`, `4827`, `4023`, `4823`, telemetry, and command acknowledgments. Verify every GCM tag before interpreting any plaintext. Reject a short body, invalid tag, invalid public point, malformed required TLV, or impossible status; never decrypt without verifying and never retry with a different cipher/key merely to make a bad packet parse. A permissive upstream authentication-failure fallback is not device behaviour and is expressly excluded from this specification.

## Payload formats

Negotiation plaintext contains untyped TLVs: one tag byte, one length byte, then exactly that many value bytes. A response generally begins with a separate status byte before these TLVs. A response may contain only that status. Request TLVs in the following exchange are in ascending tag order. Lengths depend on actual values; a UUID has length `24` hex = 36 decimal, not 24 decimal.

Session bodies begin with the untyped routing marker `a1 01 21` for requests, generally `a1 01 31` for BLE telemetry/replies. Other session values are typed TLVs: tag, one-byte length including the type byte, type, data. Known type bytes are `00` string, `01` unsigned 8-bit, `02` 16-bit LE integer, `03` 1–4-byte numeric value (four bytes in timestamps/timers here), `04` packed binary. An identity envelope uses `06`, described separately below. Never strip a presumed type byte from negotiation TLVs or the routing marker.

BLE request timestamps are a confirmed exception: trailer `fe 04 <four-byte LE Unix seconds>`, with no intervening type byte. Do not substitute the typed telemetry timestamp trailer `fe 05 03 <four bytes>`. Every normal command needs a fresh current Unix timestamp. The negotiation uses one four-byte timestamp chosen at connection start for the following stages. The module evidence says these timestamps are not cryptographic replay protection; `4022` also sets the device clock.

## Ordered connection and enrollment exchange

Here `T` means the same four-byte LE Unix timestamp chosen at the start; `U` means the persistent ASCII UUID; `S` is the serial learned in `4829`. Every row's bytes are plaintext before the appropriate GCM encryption. Every ordinary reply must have a valid tag and the expected stage identity before advancing.

| Request | Plaintext | Expected reply and contents |
|---|---|---|
| `4001` | `a1 04 T` | `4801`, static-key status plus connection parameters; observed family success is `00 a1 01 01` |
| `4003` | `a1 04 T a3 01 20 a4 02 00 f0` | `4803`, static-key capabilities; family successful body `00 a1 01 02 a2 02 fd 00 a3 01 44 a4 01 01 a5 01 02` |
| `4029` | `a1 04 T` | `4829`, static-key status plus device info |
| `4005` | `a1 04 T a3 01 20 a4 02 fd 00 a5 01 44 a6 01 02` | `4805`, static-key status, normally `00` |
| `4021` | `a1 40 <64 client public bytes>` | `4821`, static-key status then `a1 40 <64 device public bytes>` |
| `4022`, reuse | `a1 04 T a3 04 40 38 00 00 a5 04 47 4d 54 30` | `4822`, session-key status |
| `4022`, enrollment | `a1 04 T a2 24 U a3 04 40 38 00 00 a5 04 47 4d 54 30` | `4822`, session-key status |
| `4027` | `a1 04 T a2 24 U` | `4827`, session-key authorization status; distinguish `00` from `09` |
| `4023`, enrollment registration | `a1 04 T a2 24 U a3 <length(S)> S a4 01 00` | `4823`; reference captures serial from response `a1`, but it does not check status |
| `4023`, reuse compatibility finalization | `a1 01 21 a2 02 04 01 fe 04 T` | `4823`; reference sends this unusual body and advances irrespective of its status |

The `a4` in `4003` is the offered transfer ceiling `f000` LE; the `fd00` in `4005` is 253. The pinned AS220 path sends these literals. Module evidence says `4803 a2` is the granted maximum frame size, `a3` includes ECDH capability, and `a5` is authentication method; an independent implementation should reject incompatible capabilities rather than choose a different protocol. The reported AS220 path is encrypted ECDH with method 2. The success/capability example above is also reported on related modules, not a newly captured current-unit capability dump.

The literal `4022 a3` above is 14,400 seconds (`00003840`), despite the accompanying `GMT0` POSIX string. This inconsistency is in the working AS220 reference exchange; the report did not validate clock/time-zone effects. Module analysis identifies `a3` as signed LE32 UTC offset, seconds west of UTC, and `a5` as POSIX TZ. A semantically correct UTC pair would be `a3 04 00 00 00 00` with `GMT0`, but that substitution has not been explicitly tested on this AS220. It is important to document the working bytes and the uncertainty rather than silently calling `40380000` a flag or zero offset. No schedules depend on this field in the requested integration.

`4829` information uses untyped `a1` stage marker `03`, `a2` ASCII chip name (`ESP32`), `a3` ASCII protocol-library version (`0.0.0.3`), `a4` ASCII serial (17 bytes on this AS220), `a5` six MAC bytes. `a3` is not the power-station firmware version. Learn actual serial length from the TLV. Check intended serial/MAC here before the enrollment registration write. A valid `4823` may repeat the serial in untyped `a1`; it should agree, but missing data cannot be replaced by a guessed serial.

MAC-order confidence clarification: the published AS220 device-info data is consistent with the independently reported capture-address prefix, supporting direct display order rather than byte reversal. Same-module documentation also writes device-info MAC bytes in direct display order. There is no available raw-`a5`/complete-address pair for the user's current unit, so a strict six-byte comparison to the configured address has not been independently validated on that unit. A mismatch should be diagnosed with that distinction; reversing bytes speculatively is not established behaviour. The configured address selects the intended BLE peripheral, while serial is separately verified in `4829` and authorized telemetry.

Authorization is a separate result from cryptographic setup:

* Reusing the enrolled UUID should produce session-authenticated `4827` status `00` without a button. The tested AS220 reference then sends the reuse compatibility `4023` body above. It subscribes after receiving `4823`. Neither that reply's mere arrival nor ECDH success proves authorization. Require a successful `4827` and valid telemetry of the expected serial/model before declaring full operational readiness.
* With a new UUID, `4827` status `09` means a physical confirmation is pending, not successful authorization and not an instruction to repeat registration. In related-module reports it carries untyped `a1 02 <LE16 timeout seconds>`. The AS220 timeout value itself was not recorded; do not assert it is 30 or 180 seconds. The AS220 reference sends the enrollment registration `4023` after this first `4827`, gets `4823`, sends its initial subscription and then displays the one-Power-button-press instruction. It does not wait for telemetry before displaying the instruction. That is the tested prompt order. Displaying the instruction immediately after the single registration write would be a UI timing choice rather than a separately tested device requirement. Grant reception must already be active during this interval: a quick press can race the ordinary response/subscription.
* The physical press generates another session-encrypted `4827` with success status on pattern `030101` on the user's unit. Accept this event while the enrollment is pending. Do not respond by sending another `4023`: repeating registration can reset the confirmation process and consume the grant. Repeated `4822`/`4827` confirmations after completion are not a reason to replay prior auth writes.
* After successful grant, subscribe immediately and wait for identity-checked telemetry. The tested reference also sent one subscription per second while awaiting telemetry, for up to 60 seconds. Registration is not repeated by those subscriptions. If the device-reported confirmation window is shorter, respect it. An authorization grant without telemetry is not yet evidence that the intended model is usable.
* Persist the UUID before the attempt, reuse it after interruption, and preserve the existing bound UUID for this unit. Successful enrollment survived reconnects and a host reboot. The phone app's prior enrollment continued to work. A ring of 16 saved clients is reported for the module; that precise capacity was not tested on this AS220. UUID strings of 36 ASCII bytes are proven here; arbitrary token sizes and passwords are unnecessary.

**Important response-status gap:** the pinned AS220 client advances on `4801`, `4803`, `4805`, `4822`, `4827` and `4823` without checking the success byte. Its reuse `4023` body does not have the ordinary registration schema. A `4823` parameter failure is plausible and must not be reported as success. Available evidence does not establish the actual status returned to that body on the user's unit. One may reproduce the compatibility exchange while recording its verified status as such, or omit that unproven finalization after a genuine `4827 00` and verify telemetry; the latter is supported by module authorization semantics but not separately tested on AS220. Do not make successful authorization depend on accepting an error as success. Do not call `4823` an enrollment grant.

## Status handling

Response plaintexts, after decryption and after fragment reassembly if applicable, have a one-byte status before TLVs. Pushes generally have no status. The pinned AS220 captured telemetry fixture begins with a leading `00` even though described as a `c421` capture; tolerate an optional leading zero before a valid routing-marker/TLV sequence in telemetry. This does not justify stripping an arbitrary first byte or treating a nonzero response as a push.

All available full AS220 telemetry data includes routing marker `a1 01 31`; absence of that marker in a valid full AS220 snapshot has not been established. The optional byte above is the leading zero, not optionality of that known marker.

| Status | Meaning and confidence |
|---|---|
| `00` | Success; still validate required stage fields |
| `01` | Generic failure; module may use it when incoming decryption failed |
| `02` | Account check failed, module evidence |
| `04` | Required parameter check failed, module evidence |
| `09` on `4827` | Waiting for physical authorization; optionally contains window duration |
| `0a` / `0b` on `4827` | Password/authentication error in other policies; not an AS220 UUID success |
| other | Unknown failure/status, preserve numeric value; do not infer enum ordinal names |

The exact AS220 meanings of every nonzero byte are not verified. In particular, no AS220-specific mapping for `03` is established. Do not assume a generic enum position is the wire code.

A stale-key `4100` may produce `4900` with decrypted one-byte status `01` on `03000f`, rather than fragmented telemetry on `03010f` (same-module evidence). It is an error, not an empty snapshot. A second legitimate frame with the same message type can follow quickly; a rejected frame must not satisfy the wait for valid telemetry. A status-only ack proves at most receipt/handler acceptance. Application of a setting is established by a subsequent matching telemetry value; the user's report specifically found this distinction useful.

## Subscription and command encoding

After authorization, request telemetry with command `4100`, pattern `03000f`, session-encrypted plaintext:

`a1 01 21 a2 02 04 01 fe 04 <current Unix seconds LE32>`.

The `a2` is type `04` with one binary byte `01`. The first response is normally `c900`, followed by `c421` on change. No recurring keepalive is confirmed necessary after subscription. Subscribe anew on each reconnect. The user observed 2–4-second telemetry under load and about 25 seconds at idle; there is no guaranteed fixed push interval.

Ordinary setting plaintext is `a1 01 21`, followed by one of the typed setting TLVs below, followed by the untyped `fe04` timestamp trailer. Send values independently where possible; the evidence does not establish that several settings applied in one frame are atomic. An acknowledgment is usually within the same short interval as the resulting update, but only matching telemetry establishes application.

| BLE command | Tag | Encoding of data after tag/length | Meaning | Confirmed values / evidence |
|---|---|---|---|---|
| `4101` | `a2` | `01 <u8>`; TLV length 2 | AC output | 0 and 1 observed; never toggle off as a side effect of power-mode control |
| `4101` | `a3` | `03 <LE32>`; length 5 | AC output countdown, seconds | 0 sent, no independent application observation because already disabled |
| `4101` | `a4` | `02 <LE16 signed>`; length 3 | AC battery-charge cap, watts | 1100, 1000, 100, 50, 0 observed and read back |
| `4101` | `aa` | `02 <LE16 signed>`; length 3 | Smart AC no-load timeout, minutes | 240 to 0 observed |
| `4103` | `aa` | `01 <u8>`; length 2 | Maximum SoC, percent | 100 to 95 to 100 observed |
| `4103` | `a6` | `02 <LE16 signed>`; length 3 | Whole-device timeout, minutes | 720 to 0 observed |
| `4103` | `a8` | `01 <u8>`; length 2 | Remember output-port state across restart | 0 and 1 observed |
| `4090` | `a2` | `01 <u8>`; length 2 | Usage mode | 0,1,2,3 accepted; behavioural evidence for 0 and 2 |
| `4090` | `a5` | `01 <u8>`; length 2 | Backup SoC, percent | 6,99,97,100,10 observed; equal-current-SoC hold subsequently confirmed |

Expected ack message types are `4901`, `4903` and `4890`, subject to the framing flags above. Readback locations are in the following telemetry table. Explicit sample setting TLVs: 1100 W `a4 03 02 4c 04`; mode 2 `a2 02 01 02`; backup 10% `a5 02 01 0a`; AC on `a2 02 01 01`; maximum SoC 100% `aa 02 01 64`; device timeout disabled `a6 03 02 00 00`.

Secondary map limits are UI assumptions, not proven firmware constraints: AC charge commonly 100–1200 W in steps of 100, actual maximum reported at `a3+04`; this unit reports 1100, and accepts 50/0 despite that assumed minimum. Smart AC timeout options are 0,15,30,60,120,240,360,480,600,720,1440 minutes. Device timeout options are 0,30,60,120,240,360,720,1440. Max SoC mapped 80–100 in steps of 5; min SoC mapped 1,5,10,15,20; backup mapped 5–100. Do not describe these map ranges as exhaustive accepted values without device testing.

Additional mapped controls, **not confirmed in this user's BLE tests**, are `4101 a7` UltraFast (type01 bool); `4103 a2` display on (type01), `a3` brightness (type01,1/2/3), `a4` display timeout (type02 LE16;10/20/30/60/300/1800 s), `a5` temperature unit (type01;0 Celsius/1 Fahrenheit), `ab` min SoC (type01). These are factual map entries, not required controls. TOU plans, Storm Guard and custom schedules have not been validated for local autonomous use and are outside this scope.

## Telemetry fragmentation

When command fragmentation bit `80` is set, the first payload byte is a fragment descriptor outside the ciphertext. Its high nibble is a one-based fragment index; its low nibble is the total fragment count. Thus `12` then `22` are a two-part message; `13`,`23`,`33` are a three-part message. A one-part flagged message still includes this descriptor, normally `11`. An unfragmented message has no descriptor.

Concatenate the bytes after those descriptors in ascending index order, then verify/decrypt the single combined ciphertext and final GCM tag. Do not decrypt each fragment individually or count the descriptors as part of the ciphertext. Track command/channel and current connection when associating fragments; a new first fragment replaces an unfinished run of that type. There is no proven transaction sequence ID, so simultaneous overlapping snapshots of the same command cannot be safely distinguished.

Reject zero totals, index zero or index greater than total, contradictory totals, conflicting duplicate bytes, missing parts, overlong assembled bodies, and cross-session fragments. The nibbles limit a run to at most 15 pieces. Identical duplicate fragments need not produce a second telemetry event. Expire incomplete runs in bounded time; no AS220 protocol timeout for incomplete fragments is established. Reconnection discards all pending fragments. Reject an incomplete run rather than returning partial telemetry.

The reference anticipates 253-byte transfers, and family negotiation commonly returns `fd00` = 253 maximum bytes per frame (ATT MTU 256 minus 3). Do not equate a default-reported BlueZ MTU of 23 with the negotiated application capacity. Each full fragment includes 9 header bytes, one descriptor byte and one checksum byte, leaving cap minus 11 bytes of encrypted body. The 16-bit frame-length and 15-piece limits are wire-format ceilings; an implementation may choose a much smaller bounded receive budget suitable for the observed telemetry. Actual AS220 maximum snapshot size and maximum supported outbound fragmented write size have not been independently measured. Required outgoing commands fit a single frame.

## Telemetry fields

All `tag+offset` values below are offsets in **data after the type byte**, not offsets in the outer frame or in the full TLV. A packed `04` type is not part of the offset. Thus a full `a5` TLV is `a5 <length> 04 <temperature> <flow> <soc> <soh> ...`. Missing/truncated tags mean unknown fields; they do not mean zero, off, idle, or 0% SoC. Packed fields may contain uninterpreted tails; ignore unknown complete TLVs while validating their bounds.

The published AS220 telemetry capture directly establishes type `04` for each relevant packed block: its headers are `a3 0e 04`, `a4 22 04`, `a5 06 04`, `a6 0e 04`, `a7 0c 04`, `aa 04 04`, and `d9 1a 04`. Their lengths are captured facts, not permanent layout-size requirements for all firmware; the binary type marker is confirmed by that capture.

The identity `a2` envelope is special. Its first value byte in the available AS220 capture is `06`. After that byte, data begins `20 <serial length> <serial ASCII> 00 <model length> <model ASCII> <four version bytes> ...`. For a 17-byte serial and five-byte `AS220`, serial occupies data offsets 2–18, offset19 is `00`, offset20 is model length `05`, model occupies21–25, firmware bytes begin26. This explains why the reference's slices include the type/envelope byte. Parse and bounds-check the embedded lengths rather than assuming every serial/model has these lengths. Semantics of `06` and `20` beyond this observed envelope are not established. Do not misread offset20 as the first character of the model.

| Tag+offset | Width/type | Meaning | Confidence / caveat |
|---|---|---|---|
| `a2` | envelope above | Serial and model | Observed; compare to `4829` and expected `AS220` |
| `a3+00` | u8 | Working state:0 idle,1 discharge,2 charge | Observed;3 sleep/4 shutdown are secondary-map values |
| `a3+04` | LE16 integer W | Maximum normal AC charge setting | Observed constant1100; not voltage |
| `a4+00` | LE32 seconds | AC countdown setting | Secondary map; user's write0 did not establish readback |
| `a4+04` | LE16 integer W | AC charge setting | Observed follows writes |
| `a4+06` | u8 Hz | Configured AC frequency | Observed50; no live voltage/current sensor established |
| `a4+13` | LE16 minutes | Device timeout | Observed |
| `a4+15` | LE16 seconds | Display timeout | Secondary map |
| `a4+17` | u8 | Display brightness | Secondary map |
| `a4+19` | u8 | Display Fahrenheit preference | Secondary map |
| `a4+20` | u8 | UltraFast enable | Secondary map, not toggled here |
| `a4+21` | u8 | Reads1 in usage mode2 on this unit | Observed correlation; map calls it display switch, so meaning unresolved |
| `a4+22` | u8 bool | Port memory | Observed |
| `a4+23`, `+24` | u8 percent each | Max/min SoC settings | Max observed changing; min read1 |
| `a4+26` | two ASCII chars | Region-like code | Observed `US` on EU unit; do not infer mains region from it |
| `a4+28` | LE16 minutes | Smart AC timeout | Observed |
| `a5+00` | signed8 Celsius | Temperature | Observed22–26 C |
| `a5+01` | u8 enum | Battery net flow:0 idle,1 discharge,2 charge | Observed reliable; preferred basis for battery-flow state |
| `a5+02` | u8 percent | Battery SoC | Observed |
| `a5+03` | u8 percent | Battery SoH | Observed100 |
| `a6+00` | LE16 W | Total output across AC and USB | Observed, within0.5% of external plug in later tests |
| `a6+02` | LE16 W | Grid input: pass-through load plus battery charging | Observed1343 W with231 W load and1100 W charge;0 on battery |
| `a6+04` | LE16 W | DC input (solar/car) | Secondary map; not tested on current unit |
| `a6+06` | unsigned LE16 tenths of hour | Remaining duration | Observed; time-to-empty discharging, time-to-full charging |
| `a6+08` | u8 percent | Main battery SoC | Secondary map |
| `a6+10` | u8 | AC input status | Secondary map; distinguish from actual battery charging |
| `a6+11` | LE16 W | Pass-through power on tested AC-only setup | Observed equals load on grid,0 on battery; map instead calls it combined total input, so DC contribution is unresolved |
| `a7+00` | u8 bool | AC output enabled | Observed0/1 |
| `a7+01` | LE16 W | AC output | Observed; very small pass-through loads below approximately5 W may report0 |
| `a7+03` | u8 | AC plug status | Secondary map |
| `a7+04` | LE16 W | AC pass-through | Observed equals load on grid,0 on battery; supersedes secondary map's claim that it always equals grid input |
| `a8+00` | u8 | Solar/DC port state | Secondary interpretation |
| `a8+01` | LE16 W | Solar/DC input power | Reference; not stimulated on this unit |
| `aa+00` | u8 | Aggregate USB state:0 off/idle,1 output;2 input in secondary map | Observed output; aggregate, not individual-port status |
| `aa+01` | LE16 W | Aggregate USB power | Observed3–7 W; other capture summed USB-A and USB-C |
| `d9+00` | u8 | Active tariff/status-like byte | Observed4 in mode2,0 otherwise; do not force 4 into secondary map's0–3 tariff enum |
| `d9+01` | u8 | Usage mode:0 standard,1 TOU,2 self-consumption,3 custom | Observed reads back all four |
| `d9+02` | u8 percent | Backup SoC | Observed |
| `d9+03`, `+04` | u8 percent each | Max/min SoC | Observed |
| `f0+00` | u8 | Grid input actually in use | Observed1 when feeding;0 unplugged or mode2 discharge even with AC cord plugged in; not simply physical plug presence |
| `f9+00` | four bytes | Firmware version components | User saw `07 03 00 01`, likely1.0.3.7; ordering not app-verified |
| `fd` | type00, ASCII digits | Millisecond-like timestamp/transaction value | Present; secondary map says it can repeat the most recent request transaction rather than advance every frame |
| `fe` | type03 plus LE32 | Unix seconds | Typed telemetry trailer `fe 05 03 ...` |

The packed watt fields are interpreted as 16-bit integers; the secondary map generally labels them signed while the reference reads unsigned. No negative value or sentinel was observed. The valid observed values are nonnegative. Do not reinterpret a high-bit value as an enormous plausible load without treating its range/meaning as uncertain.

Preserve actual battery flow as a state; do not equate usage mode2 with ongoing discharge, or AC cord presence with battery charging. There is no confirmed instantaneous battery-watt measurement. Grid minus output can approximate battery charging only with losses and DC inputs accounted for; it is not a directly reported battery-power sensor.

Other unsolicited traffic: `4402` approximately every3 minutes contains the serial as a heartbeat. `4504` approximately every5 minutes contains packed power/status snapshots. During a272 W rack load, its `a3` four LE16 values were272,272,20,20, and an `a5` field matched backup10%. Its full layout is not established. Do not call this energy. No native energy counter was observed in53 minutes/approximately150 Wh of discharge; message `0503` reported elsewhere has not been seen on BLE here and all proposed energy field names remain speculative. Expose live power and time integrals only with their correct origin; do not advertise unsupported native counters.

## Operating-state facts and timing

Buy/charge: usage mode0 with1100 W cap supplies the load from the grid and charges the battery. The cap applies to charging, not pass-through load. One day-zero entry into mode0 below full started charging within12 seconds; ordinary setting readback was much faster. Do not treat a cap of0 as hold/off.

Sell/discharge: usage mode2 with backup10% supplied the load from the battery, with grid input falling to0. The battery stops at its reserve and naturally selects grid pass-through. This is local load supply, not grid export.

Hold: usage mode2 with backup set exactly to the **current** SoC has been confirmed to select pass-through in0–2 seconds without drift or recharge. Take a fresh, valid SoC reading when forming that request. A target above the actual SoC triggers charging toward the target. The earlier report of holding just under SoC and a possible2-point hysteresis is superseded for control purposes by the user's equal-SoC result; the exact internal hysteresis remains unknown. No fixed backup target produces hold at every SoC. The device keeps AC on across these tested transitions.

The wire protocol does not define a batch transaction for mode plus backup plus charge cap. Intermediate settings can therefore matter; plan transitions around current telemetry and confirm each relevant value. For example, changing a low backup to current SoC while remaining in mode2 requests hold without any AC-output toggle. The integration must not silently change backup above measured SoC while claiming a hold request.

Confirmed telemetry readback usually followed setting writes within0.2–1 second. Keep mode/limit readback separate from the physical battery-flow outcome and allow the observed charging startup delay. No hard firmware response deadline is established. The pinned reference uses a90-second overall negotiation allowance,15 seconds without packet progress before restarting negotiation,3 seconds between reconnect attempts, and a60-second enrollment loop with1-second subscriptions. These are reference client choices, not wire-mandated timeouts. Bounded retries with fresh keys recover occasional observed failures; preserve the same persistent UUID across attempts. Do not automatically enroll new IDs or request a button press as a response to every transient transport failure.

## Independent wire examples and validation cases

The following examples were composed independently for this document using the factual protocol constants. They are not upstream tests, and synthetic cases are not claimed as AS220 captures. They allow an implementer to verify byte ordering, lengths, checksums, tag verification and field interpretation independently of any upstream implementation.

1. **Static-key codec vector**, synthetic timestamp `01020304` (numeric Unix value0x01020304, wire bytes`04 03 02 01`). Plaintext `a10404030201`; AES-GCM ciphertext followed by tag `0a823bd1e43befe90dd68ff51e02c4af25d4e0a0c9f6`; complete `4001` request frame `ff09200003000140010a823bd1e43befe90dd68ff51e02c4af25d4e0a0c9f676`. Total32 bytes; final XOR76. This was independently computed with the standard AESGCM primitive during research. An additional factual byte vector in the pinned crypto reference is `ff09200003000140010a82d0ab535303e3aa9f0c2f9c868465bc8476f556fb7d`; independent verified decryption gives `a104ef79b569` (timestamp wire bytes`ef79b569`), using the same static key, nonce and AAD. This is a published family opener constant, not claimed as a current-unit AS220 capture.
2. **Session codec vector**, synthetic 32-byte shared secret `000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f`. Session key is00 through0f; nonce10 through1b. A1100 W write at the same synthetic timestamp has plaintext `a10121a403024c04fe0404030201`, ciphertext/tag `652f220b0c4dfaebe9d959f6c52646ca2c96ff28b055b4de4fc8ef44fd7e`, full frame `ff09280003000f4101652f220b0c4dfaebe9d959f6c52646ca2c96ff28b055b4de4fc8ef44fd7e1c`. Total40 bytes; final XOR1c. This secret is a codec input, not a real session key or an ECDH public-key test vector.
3. **Typed command boundaries**, from the confirmed1100 W command encoding: `a4 03 02 4c 04` means length3 includes type02; data`4c04` is1100 LE16. Reject `a4 02 02 4c 04` as malformed rather than consuming the leftover04 as a valid new TLV. `a5 02 01 0a` is backup10%; `a2 02 01 02` is mode2, but meanings depend on command4090.
4. **Factual telemetry bytes**, a small raw excerpt from the published device capture is `a5 06 04 1a 00 64 64 00`. Independently interpreted: data starts after04; temperature26 C, battery-flow0, SoC100%, SoH100%, one uninterpreted trailing00. This excerpt is device data, not test logic. A separate excerpt `a6 0e 04 37 00 37 00 00 00 00 00 64 01 01 37 00` contains total output55 W, grid input55 W, DC0 W and remaining duration0. The duration field is data offsets6–7, not8–9.
5. **Identity example**, constructed from the user's actual identity and the factual envelope: `a2 21 06 20 11 54 45 53 54 31 32 33 34 35 36 37 38 39 30 31 32 33 00 05 41 53 32 32 30 07 03 00 01 00 00`. The embedded17-byte serial is`TEST1234567890123`, model length5 precedes`AS220`. The final version/tail bytes use the user's reported version and capture shape; this assembled record is synthetic, not a new capture.
6. **Fragment order case**, synthetic: the single ciphertext/tag in vector2 may be partitioned after its tenth byte into two flagged packets with descriptor12 and22. The descriptors are excluded from the concatenation; the authenticated result is exactly vector2 plaintext. A missing22 yields no reading, changing22 to23 makes totals inconsistent, and mixing the next session's22 must be rejected. Each fragment needs its own complete ff09 header, length, flagged command and XOR.
7. **Authorization timeline case**, factual status semantics with synthetic timing: first `4827` status09 opens waiting; exactly one registration request is followed by the Power-button prompt; a second valid `4827` status00 on030101 grants authorization. It must trigger subscription/telemetry waiting, not a second registration. `4823` alone, `4827 09` alone, or a valid ECDH point alone does not make the device available. Reconnection with the same enrolled UUID should need no press.
8. **Failure cases**, independent requirements: changing any ciphertext/tag bit must fail GCM even when the outer XOR is repaired; changing the declared frame length or XOR must fail framing; TLV length beyond remaining bytes must reject the message; a64-byte off-curve point must not become a session; wrong-serial or non-AS220 telemetry must not enable controls; status04 must not satisfy a success wait; absent/short a5 must not become0% SoC; heartbeat4402 must not refresh battery telemetry freshness.

## Remaining gaps requiring device evidence

The main path, current-unit controls and key telemetry have high confidence. Explicit gaps are: actual status returned by the reuse compatibility4023 body; whether AS220 can skip that finalization exactly as module analysis predicts; exact current-unit4827 timeout payload; actual first-stage capability reply bytes on this unit; model-specific manufacturer-data code; semantic clock-offset correction versus the reference's working4022 literal; full packed4504 layout; whether unfragmented telemetry appears on this firmware; maximum snapshot size; fragment expiration interval; true firmware boundaries for most UI ranges; DC/solar input behaviour; negative/sentinel power values; full TOU/schedule behaviour; app-confirmed firmware version order; and any native energy counters. None is license to accept unauthenticated data, manufacture readings, or add Wi-Fi/cloud setup.
