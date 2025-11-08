use super::protocol::Message;
use super::protocol::Response;
use super::Result;
use super::Error;
use super::Sandbox;

use tracing::debug;
use tracing::error;

#[derive(Debug)]
pub(super) struct Integration {
    sandbox: Sandbox,
    state: State,
}

#[derive(Debug)]
enum State {
    NotStarted,
    AwaitingSetupStatus,
    Running,
}

impl Integration {
    pub fn new(sandbox: Sandbox) -> Self {
        Self {
            sandbox,
            state: State::NotStarted,
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        // state machine:
        // 1. Python sends "Ready" message.
        // 2. We send "SetupIntegration" message.
        // 3. Python sends "SetupComplete" or "SetupFailed".
        // 4. TBD.
        loop {
            match self.state {
                State::NotStarted => {
                    // Expect the integration to say "Ready". Then send back the SetupIntegration
                    // message.
                    match self.sandbox.recv().await? {
                        Message::Ready => {
                            self.sandbox
                                .send(Response::SetupIntegration {
                                    // TODO: we probably need an IntegrationBuilder for this, because the
                                    // integration needs this context and the Sandbox doesn't. Argh!
                                    // Hardcode for now.
                                    domain: "met".into(),
                                    name: "argh".into(),
                                    config: serde_json::json!({}),
                                })
                                .await?;
                            self.state = State::AwaitingSetupStatus;
                        }
                        m => return Err(Error::InvalidMessage {
                            expected: "Ready".into(),
                            received: m,
                        }),
                    }
                }

                State::AwaitingSetupStatus => {
                    match self.sandbox.recv().await? {
                        Message::SetupComplete {
                            name, platforms
                        } => {
                            debug!("SetupComplete: {:?}: {:?}", name, platforms);
                            todo!("next state?");
                        },

                        Message::SetupFailed{name, error, error_type, missing_package } => {
                            error!("SetupFailed: {} {} {:?} {:?}", name, error, error_type, missing_package);
                            todo!("fail properly");
                        },

                        m => return Err(Error::InvalidMessage {
                            expected: "Setup{Complete,Failed}".into(),
                            received: m,
                        }),
                    }
                },

                State::Running => todo!(),
            }
        }
    }
}
