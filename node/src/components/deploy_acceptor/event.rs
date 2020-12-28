use std::fmt::{self, Display, Formatter};

use semver::Version;
use serde::Serialize;

use super::{DeployAcceptorChainspec, Source};
use crate::{
    effect::announcements::RpcServerAnnouncement,
    types::{Deploy, NodeId},
};
use casper_types::Key;

/// `DeployAcceptor` events.
#[derive(Debug, Serialize)]
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
        maybe_chainspec: Box<Option<DeployAcceptorChainspec>>,
    },
    /// The result of the `DeployAcceptor` putting a `Deploy` to the storage component.
    PutToStorageResult {
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        is_new: bool,
    },
    /// The result of verifying `Account` exists and has meets minimum balance requirements.
    AccountVerificationResult {
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        account_key: Key,
        verified: bool,
    },
}

impl From<RpcServerAnnouncement> for Event {
    fn from(announcement: RpcServerAnnouncement) -> Self {
        match announcement {
            RpcServerAnnouncement::DeployReceived { deploy } => Event::Accept {
                deploy,
                source: Source::<NodeId>::Client,
            },
        }
    }
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Accept { deploy, source } => {
                write!(formatter, "accept {} from {}", deploy.id(), source)
            }
            Event::GetChainspecResult {
                chainspec_version,
                maybe_chainspec,
                ..
            } => {
                if maybe_chainspec.is_some() {
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
            Event::AccountVerificationResult {
                deploy,
                account_key,
                verified,
                ..
            } => {
                let prefix = if *verified { "" } else { "in" };
                write!(
                    formatter,
                    "{}valid deploy {} from account {}",
                    prefix,
                    deploy.id(),
                    account_key
                )
            }
        }
    }
}
