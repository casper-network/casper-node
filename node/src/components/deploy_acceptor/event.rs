use std::fmt::{self, Display, Formatter};

use semver::Version;

use super::{DeployAcceptorConfig, Source};
use crate::types::{Deploy, NodeId};

/// `DeployAcceptor` events.
#[derive(Debug)]
pub enum Event {
    /// The initiating event to accept a new `Deploy`.
    Accept {
        deploy: Box<Deploy>,
        source: Source<NodeId>,
    },
    /// The result of getting the chainspec from the storage component.
    GetChainspecResult {
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        chainspec_version: Version,
        maybe_deploy_config: Box<Option<DeployAcceptorConfig>>,
    },
    /// The result of the `DeployAcceptor` putting a `Deploy` to the storage component.
    PutToStorageResult {
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        is_new: bool,
    },
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Accept { deploy, source } => {
                write!(formatter, "accept {} from {}", deploy.id(), source)
            }
            Event::GetChainspecResult {
                chainspec_version,
                maybe_deploy_config,
                ..
            } => {
                if maybe_deploy_config.is_some() {
                    write!(formatter, "got chainspec at {}", chainspec_version)
                } else {
                    write!(
                        formatter,
                        "failed to get chainspec at {}",
                        chainspec_version
                    )
                }
            }
            Event::PutToStorageResult { deploy, is_new, .. } => {
                if *is_new {
                    write!(formatter, "put new {} to storage", deploy.id())
                } else {
                    write!(formatter, "had already stored {}", deploy.id())
                }
            }
        }
    }
}
