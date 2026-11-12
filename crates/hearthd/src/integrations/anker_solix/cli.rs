use std::collections::BTreeMap;
use std::io::Write;

use anyhow::Context;
use anyhow::ensure;
use clap::Args as ClapArgs;
use clap::Subcommand;
use serde::Serialize;
use uuid::Uuid;

use super::config::DeviceConfig;

#[derive(ClapArgs)]
pub struct Args {
    #[command(subcommand)]
    helper: Helper,
}

#[derive(Subcommand)]
enum Helper {
    /// Register one client identity, with a physical Power-button press.
    Enrol(Options),
    /// Check an existing client identity; never register or request a press.
    Check {
        #[command(flatten)]
        options: Options,
    },
}

#[derive(ClapArgs)]
struct Options {
    #[arg(long, default_value = "hci0")]
    adapter: String,
    /// Optional shortcut; otherwise scan and explicitly choose a candidate.
    #[arg(long)]
    address: Option<String>,
    /// Optional expected serial, checked against authenticated device information.
    #[arg(long)]
    serial: Option<String>,
    /// Required by check; enrol accepts it to resume the same attempt.
    #[arg(long)]
    client_id: Option<String>,
    #[arg(long, default_value = "s2000")]
    name: String,
}

#[cfg(target_os = "linux")]
async fn input(prompt: &str) -> anyhow::Result<String> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    eprint!("{prompt}");
    std::io::stderr().flush()?;
    // A nonblocking descriptor avoids an uncancellable blocking stdin worker
    // keeping Tokio alive after Ctrl-C at the selection prompt.
    let file = std::fs::OpenOptions::new().read(true).custom_flags(nix::libc::O_NONBLOCK).open("/dev/stdin")?;
    if file.metadata()?.is_file() {
        let mut line = String::new(); file.take(128).read_to_string(&mut line)?;
        return Ok(line.lines().next().context("input closed; helper cancelled")?.trim().to_string());
    }
    // SAFETY: AsyncFd exclusively owns this File; its descriptor remains
    // open and unchanged until AsyncFd is dropped.
    let descriptor = unsafe { tokio::io::unix::AsyncFd::register(file) }?;
    let mut line = Vec::new();
    loop {
        let mut ready = tokio::select! {
            result = descriptor.readable() => result?,
            _ = tokio::signal::ctrl_c() => anyhow::bail!("selection cancelled"),
        };
        let mut byte = [0];
        match ready.try_io(|inner| { let mut file = inner.get_ref(); file.read(&mut byte) }) {
            Ok(Ok(0)) => anyhow::bail!("input closed; helper cancelled"),
            Ok(Ok(_)) if byte[0] == b'\n' => return Ok(String::from_utf8(line)?.trim().to_string()),
            Ok(Ok(_)) => { ensure!(line.len() < 128, "selection too long"); line.push(byte[0]); }
            Ok(Err(error)) => return Err(error.into()),
            Err(_) => {}
        }
    }
}

#[derive(Serialize)]
struct PrintedDevice<'a> {
    model: &'static str,
    adapter: &'a str,
    address: &'a str,
    serial: &'a str,
    client_id: &'a str,
}

fn config_block(name: &str, device: &DeviceConfig) -> anyhow::Result<String> {
    let device = PrintedDevice { model: "s2000", adapter: &device.adapter, address: &device.address, serial: &device.serial, client_id: &device.client_id };
    let config = BTreeMap::from([("integrations", BTreeMap::from([("anker_solix", BTreeMap::from([("devices", BTreeMap::from([(name, device)]))]))]))]);
    Ok(toml::to_string_pretty(&config)?)
}

#[allow(clippy::too_many_arguments)]
async fn attempt<T: super::transport::ManagedTransport>(mut link: T, expected_serial: Option<&str>, client_id: &str, enrol: bool, cancelled: impl std::future::Future<Output = ()>, limit: std::time::Duration) -> anyhow::Result<super::protocol::Snapshot> {
    let result = tokio::select! {
        biased;
        _ = cancelled => Err(anyhow::anyhow!("helper cancelled")),
        result = tokio::time::timeout(limit, async {
            link.establish().await?;
            let mut client = super::protocol::Client::new(&mut link);
            client.authenticate(expected_serial, client_id, enrol, |serial| eprintln!("Registration sent for serial {serial}. Press this unit's Power button ONCE now; keep this connection open while authorization and telemetry are verified.")).await
        }) => result.unwrap_or_else(|_| Err(anyhow::anyhow!("helper timed out; no configuration was emitted"))),
    };
    let disconnected = link.disconnect().await;
    let snapshot = result?;
    disconnected.context("authenticated telemetry received but clean disconnect failed; no success block emitted")?;
    Ok(snapshot)
}

#[cfg(target_os = "linux")]
pub async fn run(args: &Args) -> anyhow::Result<()> {
    use std::time::Duration;
    use super::protocol;
    use super::transport;

    let (options, enrol) = match &args.helper { Helper::Enrol(options) => (options, true), Helper::Check { options } => (options, false) };
    ensure!(enrol || options.client_id.is_some(), "check requires --client-id from your existing secret config");
    ensure!(!options.name.is_empty(), "device config name must not be empty");
    let client_id = options.client_id.clone().unwrap_or_else(|| Uuid::new_v4().hyphenated().to_string());
    let address = if let Some(address) = &options.address { address.clone() } else {
        eprintln!("Scanning for advertised SOLIX S2000 / AS220 devices for 10 seconds. Release the phone app or other controller first.");
        let candidates = tokio::select! {
            result = transport::scan(&options.adapter, protocol::ADVERTISED_SERVICE, |name| { let name = name.to_ascii_uppercase(); name.contains("SOLIX S2000") || name.contains("AS220") }) => result?,
            _ = tokio::signal::ctrl_c() => anyhow::bail!("scan cancelled"),
        };
        ensure!(!candidates.is_empty(), "no nearby advertised S2000 candidates; check adapter power, range and competing connections, or use --address for a renamed unit");
        for (index, candidate) in candidates.iter().enumerate() { eprintln!("{}: {} {} (candidate; model is verified after authorization)", index + 1, candidate.address, candidate.name); }
        let selected: usize = input("Choose a device number (or Ctrl-C to cancel): ").await?.parse().context("invalid selection; helper cancelled")?;
        candidates.get(selected.checked_sub(1).context("invalid selection")?).context("selection out of range")?.address.clone()
    };
    let serial = options.serial.clone().unwrap_or_else(|| "UNVERIFIED".into());
    let device = DeviceConfig { model: "s2000".into(), adapter: options.adapter.clone(), address, serial, client_id };
    device.validate()?;
    if enrol { eprintln!("This attempt uses client ID {}. Retain it privately; to resume an interrupted attempt pass the same --client-id. No files will be edited.", device.client_id); }
    eprintln!("Connecting through system BlueZ. The unit allows one controller at a time.");
    let link = tokio::select! {
        opened = tokio::time::timeout(Duration::from_secs(15), transport::Bluez::open(&device.adapter, &device.address)) => opened.context("BlueZ access timeout")??,
        _ = tokio::signal::ctrl_c() => anyhow::bail!("discovery cancelled"),
    };
    let snapshot = attempt(link, options.serial.as_deref(), &device.client_id, enrol, async { let _ = tokio::signal::ctrl_c().await; }, Duration::from_secs(120)).await?;
    let device = DeviceConfig { serial: snapshot.serial.clone().context("verified telemetry omitted serial")?, ..device };
    eprintln!("Verified AS220 serial {} with authenticated telemetry (SoC {:?}%). {} succeeded.", device.serial, snapshot.soc, if enrol { "Enrollment" } else { "Existing identity check" });
    print!("{}", config_block(&options.name, &device)?);
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub async fn run(_args: &Args) -> anyhow::Result<()> { anyhow::bail!("Anker SOLIX S2000 helpers require Linux system BlueZ") }

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::protocol::tests::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;

    struct ManagedFake { device: FakeDevice, disconnected: Arc<AtomicBool> }
    #[async_trait::async_trait]
    impl super::super::transport::Transport for ManagedFake {
        async fn send(&mut self, bytes: &[u8]) -> anyhow::Result<()> { super::super::transport::Transport::send(&mut self.device, bytes).await }
        async fn receive(&mut self) -> anyhow::Result<Vec<u8>> { super::super::transport::Transport::receive(&mut self.device).await }
    }
    #[async_trait::async_trait]
    impl super::super::transport::ManagedTransport for ManagedFake {
        async fn establish(&mut self) -> anyhow::Result<()> { Ok(()) }
        async fn disconnect(&mut self) -> anyhow::Result<()> { self.disconnected.store(true, Ordering::SeqCst); Ok(()) }
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_and_cancellation_always_disconnect_without_a_success_result() {
        for cancelled in [false, true] {
            let disconnected = Arc::new(AtomicBool::new(false));
            let device = FakeDevice::new(false);
            device.record.lock().unwrap().suppress_readback = true;
            let link = ManagedFake { device, disconnected: disconnected.clone() };
            let result = attempt(link, Some(SERIAL), CLIENT_ID, false, async move { if !cancelled { std::future::pending::<()>().await; } }, std::time::Duration::from_secs(5)).await;
            assert!(result.is_err());
            assert!(disconnected.load(Ordering::SeqCst));
        }
    }

    #[test]
    fn success_config_is_serialized_toml_even_with_a_quoted_name() {
        let device = DeviceConfig { model: "s2000".into(), adapter: "hci0".into(), address: "00:11:22:33:44:55".into(), serial: "TESTSERIAL12345678".into(), client_id: "11111111-2222-4333-8444-555555555555".into() };
        let block = config_block("rack.\"quoted", &device).unwrap();
        let parsed: toml::Value = toml::from_str(&block).unwrap();
        assert_eq!(parsed["integrations"]["anker_solix"]["devices"]["rack.\"quoted"]["client_id"].as_str(), Some(device.client_id.as_str()));
    }
}
