use std::path::PathBuf;

use clap::Parser;
use clap::Subcommand;

#[derive(Parser)]
#[command(name = "hearthd", about = "Home automation made declarative")]
pub struct Cli {
    /// Merge these configuration files for daemon operation.
    #[arg(short, long, global = true, value_name = "FILE", default_value = "/etc/hearthd/config.toml")]
    pub config: Vec<PathBuf>,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Start the daemon (also the default when no subcommand is provided).
    Daemon,
    /// Run an integration helper without loading daemon configuration.
    #[cfg(feature = "integration_anker_solix")]
    Integration {
        #[command(subcommand)]
        integration: IntegrationCommand,
    },
}

#[cfg(feature = "integration_anker_solix")]
#[derive(Subcommand)]
pub enum IntegrationCommand {
    AnkerSolix(crate::integrations::anker_solix::cli::Args),
}

impl Cli {
    /// Helpers are dispatched before configuration, logging or HTTP startup.
    pub async fn run_helper(&self) -> anyhow::Result<bool> {
        #[cfg(feature = "integration_anker_solix")]
        if let Some(Command::Integration { integration }) = &self.command {
            match integration {
                IntegrationCommand::AnkerSolix(args) => crate::integrations::anker_solix::cli::run(args).await?,
            }
            return Ok(true);
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_and_explicit_daemon_invocations_parse() {
        for args in [vec!["hearthd", "--config", "base.toml", "--config", "secrets.toml"], vec!["hearthd", "daemon", "--config", "base.toml", "--config", "secrets.toml"]] {
            assert_eq!(Cli::try_parse_from(args).unwrap().config.len(), 2);
        }
    }

    #[test]
    fn helper_command_tracks_the_integration_feature() {
        let result = Cli::try_parse_from(["hearthd", "integration", "anker-solix", "enrol"]);
        assert_eq!(result.is_ok(), cfg!(feature = "integration_anker_solix"));
    }
}
