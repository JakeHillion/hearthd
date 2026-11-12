#[cfg(feature = "integration_anker_solix")]
pub mod anker_solix;
#[cfg(feature = "integration_apc")]
pub mod apc;
#[cfg(feature = "integration_ecoflow")]
pub mod ecoflow;
#[cfg(feature = "integration_metno")]
pub mod metno;
#[cfg(feature = "integration_mqtt")]
pub mod mqtt;
#[cfg(feature = "integration_snapcast")]
pub mod snapcast;
#[cfg(feature = "integration_wake_on_lan")]
pub mod wake_on_lan;

/// Helpers an integration provides for use outside the daemon, as
/// `hearthd integration <name> ...`.
#[derive(clap::Subcommand)]
pub enum Command {
    #[cfg(feature = "integration_anker_solix")]
    AnkerSolix(anker_solix::cli::Args),
}

impl Command {
    pub async fn run(self) -> anyhow::Result<()> {
        match self {
            #[cfg(feature = "integration_anker_solix")]
            Command::AnkerSolix(args) => anker_solix::cli::run(&args).await,
        }
    }
}
