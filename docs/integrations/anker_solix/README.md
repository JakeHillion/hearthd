# Anker SOLIX S2000

The `anker_solix` integration initially supports `model = "s2000"` (AS220)
over Linux system BlueZ. Other SOLIX models may need other transports, including
Modbus. This integration owns one persistent connection per configured unit;
it uses no cloud account, Wi-Fi provisioning or MQTT. It does not implement
Matter networking or certification.

This is an independently written implementation of the separately researched
[wire specification](protocol.md). The specification records evidence and
uncertainties. Automated validation uses synthetic devices and cryptographic
vectors; real-device validation is still required before landing.

## Configuration and system access

The default build enables `integration_anker_solix`. Disable default features
to omit it, or select it explicitly with Cargo's feature flags. Linux builds
need `pkg-config` and D-Bus development headers (`libdbus-1-dev` on Debian).
The Nix build and development shell supply these dependencies.

Use your unit's actual values. This example is entirely synthetic:

```toml
[integrations.anker_solix.devices.rack]
model = "s2000"
adapter = "hci0"
address = "00:11:22:33:44:55"
serial = "TEST1234567890123"
client_id = "11111111-2222-4333-8444-555555555555"
```

The map key supplies the display name; serial supplies the stable node key.
Adapters are per device. Repeated addresses or serials, an empty device map,
unknown models and malformed identifiers fail configuration validation.
Keep `client_id` in a private secrets file using the normal config merging or
imports mechanism. The daemon's configuration Debug output redacts this field.
For an already bound unit, retain the existing client ID: normal startup and
reconnect never generate or register a new identity.

Run the system `bluetoothd` and enable the chosen radio using system settings.
Grant the hearthd service user permission to call `org.bluez` on the system
D-Bus. Do not run hearthd as root or start a private bus/daemon. Discovery is
reference-counted through BlueZ; hearthd does not power-cycle the adapter or
change shared discovery filters. A cold BlueZ device cache is populated by a
bounded discovery session before connecting to a configured address.

For NixOS, enable `hardware.bluetooth.enable = true`. If your policy does not
already authorize the daemon, use a stable service user and an explicit policy:

```nix
{ pkgs, lib, ... }: {
  hardware.bluetooth.enable = true;
  users.groups.hearthd = { };
  users.users.hearthd = { isSystemUser = true; group = "hearthd"; };
  services.dbus.packages = [
    (pkgs.writeTextDir "share/dbus-1/system.d/hearthd-bluez.conf" ''
      <!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
        "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
      <busconfig><policy user="hearthd">
        <allow send_destination="org.bluez"/>
      </policy></busconfig>
    '')
  ];
  systemd.services.hearthd = {
    after = [ "bluetooth.service" ];
    wants = [ "bluetooth.service" ];
    serviceConfig = { DynamicUser = lib.mkForce false; User = "hearthd"; Group = "hearthd"; };
  };
}
```

This policy trusts that user to access BlueZ, so restrict access to its account
and secret configuration. Inspect your distribution's installed D-Bus policy
before adding a rule. No service invocation migration is needed:
`hearthd --config base.toml --config secrets.toml` still starts the daemon;
`hearthd daemon --config ...` is also accepted.

## Enrollment and checking a binding

Stop other controllers first. The unit permits one BLE connection; while the
phone app, Python controller or hearthd holds it, it may disappear from scans.
Helpers load no daemon configuration, start no other integrations and bind no
HTTP port. Optional helper commands disappear when the feature is disabled.

For a new registration:

```sh
hearthd integration anker-solix enrol --adapter hci0 --name rack
```

The helper scans for S2000/AS220 names with the family service, lists candidates
and requires an explicit selection even when there is one. A candidate's name
does not prove its model. For a renamed unit, use `--address 00:11:22:33:44:55`.
The authenticated device-info serial is shown in the registration prompt;
`--serial TEST1234567890123` optionally checks an expected serial before any
registration. Conclusive AS220 model checking happens in authorized telemetry.

One UUID is generated and displayed privately on stderr for that attempt.
Retain it to resume an interrupted attempt with `enrol --client-id UUID`.
No identity or configuration file is written automatically. After the helper
sends registration once, press **Power once when prompted** and leave it
connected. It waits for an authenticated authorization grant and telemetry of
the expected serial/model. ECDH completion or a registration ACK is insufficient.
Timeout, Ctrl-C and failed checks disconnect and emit no success TOML.

For an existing binding, use its private UUID instead:

```sh
hearthd integration anker-solix check --adapter hci0 \
  --address 00:11:22:33:44:55 --serial TEST1234567890123 \
  --client-id 11111111-2222-4333-8444-555555555555 --name rack
```

`check` never registers a client or requests a button press. It also requires
authenticated telemetry. Successful helpers print a TOML device block to stdout;
all candidate lists, progress, prompts and errors go to stderr. Add successful
output to the private secrets configuration yourself. Be aware that a literal
UUID supplied on a command line can remain in shell history or process listings.

## Data model and commands

| Endpoint | Clusters | Meaning |
|---|---|---|
| 1 | PowerSource, TemperatureMeasurement | Battery SoC, native charge state, duration, temperature |
| 2 | OnOff, ElectricalPowerMeasurement | AC output enable and AC load |
| 3 | ElectricalPowerMeasurement | Grid input including pass-through plus battery charging |
| 4 | ElectricalPowerMeasurement | Reported DC input; interpretation needs solar/car testing |
| 5 | ElectricalPowerMeasurement | Aggregate USB output, with no invented per-port channels |
| 6 | ElectricalPowerMeasurement | Reported total AC plus USB output |
| 7 | ModeSelect, SolixSettings | Native mode, limits and confirmed diagnostics |

Electrical power is in milliwatts, **imports positive and outputs negative**
relative to the station. The mixed AC/USB total uses PowerMode Unknown.
Unknown voltage/current/frequency remain null; the configured frequency is not
a live measurement. Device watt values with the high bit set have unresolved
signed/sentinel meaning and are left unknown. No native energy counters are
known: no Wh estimate, watt integration, inferred battery watts or loss model
is created. Grid input and total load are never labelled battery power.

Battery percentages use Matter's half-percent units (73% becomes 146), times
are seconds (one reported tenth-hour becomes 360 seconds), and temperature is
hundredths Celsius. Native flow 2 means charging, 0/1 means not charging; 100%
alone never implies full-charge state. Remaining duration maps to time-to-empty
only when discharging, or time-to-full only when charging. Battery flow, health
and observed AC pass-through watts are read-only diagnostics in SolixSettings.

`SolixSettings` uses vendor cluster **0xFC09**, leaving the proposed Dyson
0xFC06--0xFC08 allocation free. Writable attributes are `charge_cap_watts`,
`backup_soc` and `maximum_soc`. The implementation conservatively accepts
50 W through the device-reported maximum for the charge cap, 5..100% backup
(at most the reported maximum SoC), and 80..100% maximum SoC (at least the
reported backup). These are integration guardrails, not exhaustive firmware
limits. Zero watts is rejected because it does **not** stop the charger.
ModeSelect advertises only Standard (0) and Self-consumption (2); use the
ChangeToMode command, not a write to its read-only current_mode attribute.

Examples using the discovered name `rack`:

```sh
curl -X POST http://127.0.0.1:8565/v1/nodes/rack/command \
  -H 'content-type: application/json' \
  -d '{"endpoint":2,"command":{"cluster":"OnOff","command":"On"}}'
curl -X POST http://127.0.0.1:8565/v1/nodes/rack/command \
  -H 'content-type: application/json' \
  -d '{"endpoint":7,"command":{"cluster":"ModeSelect","command":{"ChangeToMode":{"new_mode":2}}}}'
curl -X POST http://127.0.0.1:8565/v1/nodes/rack/write \
  -H 'content-type: application/json' \
  -d '{"endpoint":7,"write":{"cluster":"SolixSettings","attribute":"charge_cap_watts","value":100}}'
```

Commands serialize per device. HTTP 202 means queued, not applied. Only
subsequent matching telemetry confirms a write; ACK alone cannot publish
optimistic success. Connection failures, rejected requests and unconfirmed
writes use the existing tracing logs. Confirmations are logged at info level;
individual pending writes are logged at debug level. Enable the
`hearthd::integrations::anker_solix` logging target at debug for that detail.

The integration announces a node through the existing Publisher only after
fresh authenticated SoC and verified serial/model telemetry. Fresh SoC expires
after 60 seconds and is reset per connection. On stale or invalid battery data,
or a disconnected link, the integration withdraws the node, so it disappears
from `/v1/state` and uses the API's existing absent-node behavior. Fresh data
reannounces the same serial-based LocalKey and therefore the same node ID.
Unrelated measurements and heartbeats cannot keep an old battery snapshot
fresh. Invalid/truncated reported groups invalidate previous readings.

The local command guard also checks freshness and rejects requests queued
against an invalidated snapshot or an earlier connection. Queued commands
expire after five seconds; an unconfirmed write logs a warning after ten
seconds. These are implementation deadlines, not measured firmware guarantees.

The controller may combine native settings into buy/hold/sell intent outside
this integration. Standard mode plus a cap charges; self-consumption plus a
low backup supplies the local load from battery. Setting backup equal to a
fresh current SoC can hold, while setting it **above** current SoC can charge.
There is no atomic multi-setting operation. Changing usage mode does not
toggle AC output. Start/reconnect/shutdown send no operating-setting writes,
and shutdown does not restore charging: a unit left discharging stays in its
native mode. The negotiation's clock/timezone bytes reproduce the known AS220
exchange; the documented offset/GMT0 inconsistency needs hardware verification.

## Connection cleanup and daemon lifecycle

Managed link exit, including a physical disconnect, protocol failure or local
integration cancellation, withdraws the node and explicitly calls BlueZ
Disconnect. Enrollment/check helpers also await disconnect on success,
failure, timeout and cancellation. Neither path writes operating settings as
cleanup.

The existing daemon engine does not await every integration's asynchronous
shutdown when its runtime stops. This integration requests worker cancellation
when dropped, but cannot guarantee an awaited Bluetooth Disconnect during a
daemon-wide SIGTERM/runtime teardown. Hardware handover after stopping the
daemon must be verified: check that the unit becomes available to another
controller. Any stronger awaited daemon-shutdown guarantee would require a
separate lifecycle design.

## Hardware verification before landing

Run these only in an agreed later device session. Preserve the existing UUID
for an already enrolled unit and record settings before trying writes.

1. Release the phone/other controller. Run `check` with the existing UUID and
   expected serial; verify no button press and no operating-setting change.
   Record the static device-info MAC versus the BLE address: direct order is
   supported by family evidence, but full equality is not a success gate
   because it has not been verified on this unit.
   Our reuse path omits the uncertain compatibility 4023 finalization after a
   genuine 4827 grant. Verify telemetry works on the actual firmware.
2. For a genuinely new registration, exercise scan/selection, a fast and a
   delayed Power press, cancellation and a timeout. Confirm one registration,
   no stdout block on failure, and that a later check reuses the same UUID.
3. Start a minimal private `*.local.toml` daemon config manually, recording its
   exact PID. Inspect `/v1/ping`, `/v1/state` and SoC/time/temperature/power
   units and signs against the display/external meter. Confirm grid input
   includes pass-through, total equals native AC plus USB load, and null fields
   remain null. DC/solar interpretations are still unverified.
4. Issue one reversible limit or mode command at a time with the API examples.
   Confirm 202 precedes native readback, readback updates the clusters and
   logs confirmation, and unconfirmed/rejected writes produce log warnings. Save and restore settings
   explicitly after testing; shutdown performs no restoration.
5. Interrupt Bluetooth or disconnect the device while idle and during a
   pending command. Confirm the node is withdrawn from `/v1/state`, requests
   use the existing absent-node response, reconnect is bounded, and fresh
   identity-checked battery data reannounces the same node ID. Check that no
   command is replayed and the client UUID is unchanged. Stop the recorded PID
   normally and test whether the connection is released, recording the existing
   daemon lifecycle limitation described above.

Unverified areas remain explicitly outside the control surface: modes 1/3,
TOU/custom schedules, native energy counters, true firmware limit boundaries,
negative/sentinel power readings, and full DC/solar semantics. See the factual
specification for capability/status/clock and fragment-size evidence gaps.
