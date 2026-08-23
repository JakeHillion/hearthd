//! `hearthd integration dyson login`
//!
//! Interactive one-time login to the Dyson cloud API. Prints a TOML snippet
//! that can be pasted into a `hearthd` secrets config.

use std::io::Write;
use std::io::{self};

use anyhow::Context;
use anyhow::Result;

use super::cloud::DeviceInfo;
use super::cloud::DysonAccount;

/// Run the interactive login flow and print a TOML snippet.
pub async fn run_login_interactive(email: &str, region: &str) -> Result<()> {
    let mut account = DysonAccount::new().context("failed to create Dyson account client")?;

    account
        .request_email_otp(email, region)
        .await
        .context("failed to request email OTP")?;

    print!("Enter the OTP code sent to {}: ", email);
    io::stdout().flush().context("failed to flush stdout")?;
    let mut otp = String::new();
    io::stdin()
        .read_line(&mut otp)
        .context("failed to read OTP from stdin")?;
    let otp = otp.trim();

    print!("Enter your Dyson account password: ");
    io::stdout().flush().context("failed to flush stdout")?;
    let password = rpassword::read_password().context("failed to read Dyson account password")?;
    let password = password.trim();

    account
        .verify_email_otp(email, password, otp)
        .await
        .context("failed to verify OTP")?;

    let devices = account.devices().await.context("failed to list devices")?;
    if devices.is_empty() {
        println!("# No devices found in this Dyson account.");
        return Ok(());
    }

    println!("# Paste this under [integrations.dyson.devices.<name>] in your secrets config:\n");
    for info in devices {
        print_device_snippet(&info);
    }
    Ok(())
}

fn print_device_snippet(info: &DeviceInfo) {
    println!("serial = {:?}", info.serial);
    println!("credential = {:?}", info.credential);
    println!("device_type = {:?}", info.device_type);
    println!("# host = \"IP_ADDRESS\"; # set to the device's local IP");
    println!("# name = {:?}", info.name);
    println!();
}
