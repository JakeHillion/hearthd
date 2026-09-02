//! End-to-end tests that run the real Python shim against the live met.no API
//! and check it against hearthd's own met.no integration.
//!
//! These are the only tests that exercise the whole bridge: the sandbox
//! handshake, the coordinator's state updates, the unit plumbing and the
//! translation to Matter clusters. Both integrations here read the same
//! upstream forecast by different routes — one through Home Assistant's `met`
//! component in Python, one through [`crate::integrations::metno`] in Rust —
//! so the native integration acts as an independent oracle. A units bug in the
//! shim shows up as a systematic disagreement between them; nothing in the
//! unit tests could have caught reading met.no's km/h wind as m/s, but this
//! does.
//!
//! # Running them
//!
//! They are `#[ignore]`d because they need the network, a Python interpreter
//! with the Home Assistant dependencies, and the `vendor/ha-core` checkout:
//!
//! ```text
//! nix develop --command \
//!   cargo test -p hearthd --lib ha::live_tests -- --ignored --test-threads=1 --nocapture
//! ```
//!
//! `--test-threads=1` is not optional: the shim resolves `python/runner.py`
//! and `vendor/ha-core` relative to the process working directory, so the test
//! has to `chdir` to the workspace root, and that is process-global.
//! Un-ignoring these without fixing that path handling would make the rest of
//! the suite order-dependent.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use tokio::sync::mpsc;

use crate::engine::FromIntegrationMessage;
use crate::engine::Integration as _;
use crate::engine::NodeId;
use crate::engine::NodeIdAllocator;
use crate::integrations::ha::HaIntegration;
use crate::integrations::metno::MetnoIntegration;
use crate::integrations::metno::Site;
use crate::matter::Cluster;

/// Oslo, matching the coordinates the shim's `IntegrationConfig::default()`
/// hardcodes, so both integrations describe the same place.
const LATITUDE: f64 = 59.9139;
const LONGITUDE: f64 = 10.7522;
const ELEVATION_M: f64 = 23.0;

/// Long enough for the Python interpreter to boot, import `homeassistant`,
/// and complete one coordinator refresh against api.met.no.
const COLLECT_FOR: Duration = Duration::from_secs(45);

/// What a node looked like after replaying every message about it.
#[derive(Debug)]
struct ObservedNode {
    integration: String,
    clusters: HashMap<String, Cluster>,
}

/// Replay the message stream into the same shape the engine would build.
async fn collect(
    rx: &mut mpsc::Receiver<FromIntegrationMessage>,
    until: Duration,
) -> HashMap<NodeId, ObservedNode> {
    let mut nodes: HashMap<NodeId, ObservedNode> = HashMap::new();
    let deadline = tokio::time::Instant::now() + until;

    while let Ok(Some(msg)) = tokio::time::timeout_at(deadline, rx.recv()).await {
        match msg {
            FromIntegrationMessage::NodeAdded { node_id, node } => {
                let clusters = node
                    .endpoints
                    .values()
                    .flat_map(|e| e.clusters.clone())
                    .collect();
                nodes.insert(
                    node_id,
                    ObservedNode {
                        integration: node.integration,
                        clusters,
                    },
                );
            }
            FromIntegrationMessage::AttributeChanged {
                node_id, cluster, ..
            } => {
                if let Some(node) = nodes.get_mut(&node_id) {
                    node.clusters.insert(cluster.name().to_string(), cluster);
                }
            }
            FromIntegrationMessage::NodeRemoved { node_id } => {
                nodes.remove(&node_id);
            }
        }
    }

    nodes
}

fn find_by_integration<'a>(
    nodes: &'a HashMap<NodeId, ObservedNode>,
    integration: &str,
) -> &'a ObservedNode {
    nodes
        .values()
        .find(|n| n.integration == integration)
        .unwrap_or_else(|| {
            panic!(
                "no node from '{integration}'; saw {:?}",
                nodes.values().map(|n| &n.integration).collect::<Vec<_>>()
            )
        })
}

/// Assert two readings of the same quantity agree, allowing for the two
/// integrations sampling the forecast at slightly different points.
///
/// The tolerance is deliberately loose: this is checking for a wrong *scale*,
/// not for equality. A unit error is a factor of 3.6 (km/h vs m/s) or an
/// offset of 273 (K vs °C); sampling noise is a fraction of a percent.
#[track_caller]
fn assert_same_scale(quantity: &str, shim: Option<f64>, native: Option<f64>, tolerance: f64) {
    let (Some(shim), Some(native)) = (shim, native) else {
        panic!("{quantity}: shim={shim:?} native={native:?}, expected both to be reported");
    };

    let allowed = tolerance.max(native.abs() * 0.2);
    assert!(
        (shim - native).abs() <= allowed,
        "{quantity}: shim reported {shim}, hearthd's own met.no integration \
         reported {native}; difference exceeds {allowed}. A gap this large is \
         a unit conversion error, not sampling noise."
    );
}

fn temperature(node: &ObservedNode) -> Option<f64> {
    match node.clusters.get("TemperatureMeasurement")? {
        Cluster::TemperatureMeasurement(c) => c.measured_value.map(|v| v as f64 / 100.0),
        _ => None,
    }
}

fn pressure(node: &ObservedNode) -> Option<f64> {
    match node.clusters.get("PressureMeasurement")? {
        Cluster::PressureMeasurement(c) => c.measured_value.map(|v| v as f64),
        _ => None,
    }
}

fn humidity(node: &ObservedNode) -> Option<f64> {
    match node.clusters.get("RelativeHumidityMeasurement")? {
        Cluster::RelativeHumidityMeasurement(c) => c.measured_value.map(|v| v as f64 / 100.0),
        _ => None,
    }
}

fn wind_speed(node: &ObservedNode) -> Option<f64> {
    match node.clusters.get("WindMeasurement")? {
        Cluster::WindMeasurement(c) => c.speed.map(|v| v as f64 / 100.0),
        _ => None,
    }
}

/// Move to the workspace root, which is where the shim expects to find
/// `python/runner.py` and `vendor/ha-core`.
fn chdir_to_workspace_root() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/hearthd is two levels below the workspace root");
    std::env::set_current_dir(root).expect("chdir to workspace root");
    assert!(
        Path::new("vendor/ha-core/homeassistant").is_dir(),
        "vendor/ha-core is not checked out; this test cannot run without it"
    );
}

#[tokio::test]
#[ignore = "needs the network, HA_PYTHON_INTERPRETER and a vendor/ha-core checkout"]
async fn shim_agrees_with_the_native_metno_integration() {
    chdir_to_workspace_root();

    let (tx, mut rx) = mpsc::channel(1024);
    let node_ids = NodeIdAllocator::for_test();

    let mut shim = HaIntegration::new("ha".to_string());
    shim.setup(tx.clone(), node_ids.clone())
        .await
        .expect("HA shim setup");

    let mut native = MetnoIntegration::new(vec![Site {
        name: "oslo".to_string(),
        latitude: LATITUDE,
        longitude: LONGITUDE,
        elevation_m: Some(ELEVATION_M),
    }]);
    native
        .setup(tx.clone(), node_ids)
        .await
        .expect("metno setup");

    // The collector ends on the deadline, not on the channel closing.
    drop(tx);
    let nodes = collect(&mut rx, COLLECT_FOR).await;

    // The shim names its integration after the sandbox instance, not "ha".
    let shim_node = find_by_integration(&nodes, "met_oslo");
    let native_node = find_by_integration(&nodes, "metno");

    // Both must describe a node of the same shape, or downstream consumers
    // have to care which integration a weather node came from.
    let mut shim_clusters: Vec<_> = shim_node.clusters.keys().cloned().collect();
    let mut native_clusters: Vec<_> = native_node.clusters.keys().cloned().collect();
    shim_clusters.sort();
    native_clusters.sort();
    assert_eq!(
        shim_clusters, native_clusters,
        "the two integrations publish different cluster sets"
    );

    assert_same_scale(
        "temperature (°C)",
        temperature(shim_node),
        temperature(native_node),
        2.0,
    );
    assert_same_scale(
        "pressure (hPa)",
        pressure(shim_node),
        pressure(native_node),
        10.0,
    );
    assert_same_scale(
        "humidity (%)",
        humidity(shim_node),
        humidity(native_node),
        10.0,
    );
    // The assertion that would have caught reading km/h as m/s.
    assert_same_scale(
        "wind speed (m/s)",
        wind_speed(shim_node),
        wind_speed(native_node),
        1.5,
    );
}
