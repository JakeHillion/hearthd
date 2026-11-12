use std::collections::HashMap;

use super::protocol::Snapshot;
use crate::matter::*;

pub const BATTERY: u16 = 1;
pub const AC_OUTPUT: u16 = 2;
pub const GRID_INPUT: u16 = 3;
pub const DC_INPUT: u16 = 4;
pub const USB: u16 = 5;
pub const TOTAL_OUTPUT: u16 = 6;
pub const SETTINGS: u16 = 7;

fn power(watts: Option<u16>, mode: PowerMode, output: bool) -> Cluster {
    Cluster::ElectricalPowerMeasurement(ElectricalPowerMeasurementCluster { power_mode: mode, active_power: watts.map(|value| i64::from(value) * if output { -1000 } else { 1000 }), ..Default::default() })
}

pub fn node(serial: &str, name: &str, snapshot: &Snapshot) -> Node {
    let charge_state = snapshot.flow.map(|flow| match flow { 2 => BatChargeState::IsCharging, 0 | 1 => BatChargeState::IsNotCharging, _ => BatChargeState::Unknown });
    let battery = Endpoint::from_clusters([
        Cluster::PowerSource(PowerSourceCluster { status: if snapshot.soc.is_some() { PowerSourceStatus::Active } else { PowerSourceStatus::Unspecified }, description: "S2000 battery".into(), bat_percent_remaining: snapshot.soc.map(|value| value * 2), bat_charge_state: charge_state, bat_time_remaining: (snapshot.flow == Some(1)).then_some(snapshot.remaining_seconds).flatten(), bat_time_to_full_charge: (snapshot.flow == Some(2)).then_some(snapshot.remaining_seconds).flatten(), ..Default::default() }),
        Cluster::TemperatureMeasurement(TemperatureMeasurementCluster { measured_value: snapshot.temperature.map(|value| i16::from(value) * 100) }),
    ]).with_device_types([DeviceType::PowerSource, DeviceType::TemperatureSensor]);
    let mut ac = vec![power(snapshot.ac_watts, PowerMode::Ac, true)];
    if let Some(on_off) = snapshot.ac_on { ac.push(Cluster::OnOff(OnOffCluster { on_off })); }
    let settings = Endpoint::from_clusters([
        Cluster::ModeSelect(ModeSelectCluster { description: "S2000 native usage mode".into(), supported_modes: vec![ModeOption { label: "Standard".into(), mode: 0 }, ModeOption { label: "Self-consumption".into(), mode: 2 }], current_mode: snapshot.mode.filter(|mode| [0, 2].contains(mode)) }),
        Cluster::SolixSettings(SolixSettingsCluster { charge_cap_watts: snapshot.charge_cap_watts, maximum_charge_cap_watts: snapshot.max_charge_cap_watts, backup_soc: snapshot.backup_soc, maximum_soc: snapshot.max_soc, battery_flow: snapshot.flow, battery_health_percent: snapshot.health, ac_pass_through_watts: snapshot.pass_through_watts }),
    ]).with_device_types([DeviceType::ModeSelect]);
    let ac_types = if snapshot.ac_on.is_some() { vec![DeviceType::OnOffPlugInUnit, DeviceType::ElectricalSensor] } else { vec![DeviceType::ElectricalSensor] };
    Node { key: LocalKey::from(serial), name: Some(name.into()), endpoints: HashMap::from([(BATTERY, battery), (AC_OUTPUT, Endpoint::from_clusters(ac).with_device_types(ac_types)), (GRID_INPUT, Endpoint::from_clusters([power(snapshot.grid_watts, PowerMode::Ac, false)]).with_device_types([DeviceType::ElectricalSensor])), (DC_INPUT, Endpoint::from_clusters([power(snapshot.dc_watts, PowerMode::Dc, false)]).with_device_types([DeviceType::ElectricalSensor])), (USB, Endpoint::from_clusters([power(snapshot.usb_watts, PowerMode::Dc, true)]).with_device_types([DeviceType::ElectricalSensor])), (TOTAL_OUTPUT, Endpoint::from_clusters([power(snapshot.total_watts, PowerMode::Unknown, true)]).with_device_types([DeviceType::ElectricalSensor])), (SETTINGS, settings)]) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::protocol::tests::*;

    #[test]
    fn matter_units_signs_types_and_unknown_measurements_are_preserved() {
        let snapshot = Snapshot::decode(&telemetry()).unwrap();
        let node = node(SERIAL, "Rack", &snapshot);
        assert_eq!(node.key.as_str(), SERIAL);
        for endpoint in node.endpoints.values() { assert!(endpoint.missing_mandatory_clusters().is_empty()); }
        let Cluster::PowerSource(battery) = &node.endpoints[&BATTERY].clusters["PowerSource"] else { panic!() };
        assert_eq!(battery.bat_percent_remaining, Some(146));
        assert_eq!(battery.bat_time_to_full_charge, Some(5400));
        assert_eq!(battery.bat_time_remaining, None);
        assert_eq!(battery.bat_charge_state, Some(BatChargeState::IsCharging));
        let Cluster::TemperatureMeasurement(temperature) = &node.endpoints[&BATTERY].clusters["TemperatureMeasurement"] else { panic!() };
        assert_eq!(temperature.measured_value, Some(2600));
        for (endpoint, expected) in [(GRID_INPUT, 1_343_000), (AC_OUTPUT, -224_000), (USB, -7000), (TOTAL_OUTPUT, -231_000)] {
            let Cluster::ElectricalPowerMeasurement(power) = &node.endpoints[&endpoint].clusters["ElectricalPowerMeasurement"] else { panic!() };
            assert_eq!(power.active_power, Some(expected));
            assert_eq!((power.voltage, power.active_current, power.frequency), (None, None, None));
        }
        let mut full = snapshot; full.soc = Some(100); full.flow = Some(0);
        let node = super::node(SERIAL, "Rack", &full);
        let Cluster::PowerSource(battery) = &node.endpoints[&BATTERY].clusters["PowerSource"] else { panic!() };
        assert_eq!(battery.bat_charge_state, Some(BatChargeState::IsNotCharging));
        assert_eq!(battery.bat_percent_remaining, Some(200));
    }
}
