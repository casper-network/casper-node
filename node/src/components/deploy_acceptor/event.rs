use std::fmt::{self, Display, Formatter};

use serde::Serialize;

use super::Source;
use crate::{
    components::deploy_acceptor::Error,
    effect::{announcements::RpcServerAnnouncement, Responder},
    types::{Block, Deploy, NodeId, Timestamp},
};

use casper_execution_engine::core::engine_state::executable_deploy_item::{
    ContractIdentifier, ContractPackageIdentifier,
};
use casper_hashing::Digest;
use casper_types::{account::Account, Contract, ContractPackage, U512};

/// A utility struct to hold duplicated information across events.
#[derive(Debug, Serialize)]
pub(crate) struct EventMetadata {
    pub(crate) deploy: Box<Deploy>,
    pub(crate) source: Source<NodeId>,
    pub(crate) maybe_responder: Option<Responder<Result<(), Error>>>,
}

impl EventMetadata {
    pub(crate) fn new(
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    ) -> Self {
        EventMetadata {
            deploy,
            source,
            maybe_responder,
        }
    }

    pub(crate) fn deploy(&self) -> &Deploy {
        &self.deploy
    }

    pub(crate) fn source(&self) -> &Source<NodeId> {
        &self.source
    }

    pub(crate) fn take_maybe_responder(self) -> Option<Responder<Result<(), Error>>> {
        self.maybe_responder
    }
}

/// `DeployAcceptor` events.
#[derive(Debug, Serialize)]
pub(crate) enum Event {
    /// The initiating event to accept a new `Deploy`.
    Accept {
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    },
    /// The result of the `DeployAcceptor` putting a `Deploy` to the storage component.
    PutToStorageResult {
        event_metadata: EventMetadata,
        is_new: bool,
        verification_start_timestamp: Timestamp,
    },
    /// The InvalidDeployResult event
    InvalidDeployResult {
        event_metadata: EventMetadata,
        error: Error,
        verification_start_timestamp: Timestamp,
    },
    GetBlockResult {
        event_metadata: EventMetadata,
        maybe_block: Box<Option<Block>>,
        verification_start_timestamp: Timestamp,
    },
    GetAccountResult {
        event_metadata: EventMetadata,
        prestate_hash: Digest,
        maybe_account: Option<Account>,
        verification_start_timestamp: Timestamp,
    },
    GetBalanceResult {
        event_metadata: EventMetadata,
        prestate_hash: Digest,
        maybe_balance_value: Option<U512>,
        verification_start_timestamp: Timestamp,
    },
    VerifyPaymentLogic {
        event_metadata: EventMetadata,
        prestate_hash: Digest,
        verification_start_timestamp: Timestamp,
    },
    VerifySessionLogic {
        event_metadata: EventMetadata,
        prestate_hash: Digest,
        verification_start_timestamp: Timestamp,
    },
    GetContractResult {
        event_metadata: EventMetadata,
        prestate_hash: Digest,
        is_payment: bool,
        contract_identifier: ContractIdentifier,
        maybe_contract: Option<Contract>,
        verification_start_timestamp: Timestamp,
    },
    GetContractPackageResult {
        event_metadata: EventMetadata,
        prestate_hash: Digest,
        is_payment: bool,
        contract_package_identifier: ContractPackageIdentifier,
        maybe_contract_package: Option<ContractPackage>,
        verification_start_timestamp: Timestamp,
    },
    VerifyDeployCryptographicValidity {
        event_metadata: EventMetadata,
        verification_start_timestamp: Timestamp,
    },
}

impl From<RpcServerAnnouncement> for Event {
    fn from(announcement: RpcServerAnnouncement) -> Self {
        match announcement {
            RpcServerAnnouncement::DeployReceived { deploy, responder } => Event::Accept {
                deploy,
                source: Source::<NodeId>::Client,
                maybe_responder: responder,
            },
        }
    }
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Accept { deploy, source, .. } => {
                write!(formatter, "accept {} from {}", deploy.id(), source)
            }
            Event::PutToStorageResult {
                event_metadata,
                is_new,
                ..
            } => {
                if *is_new {
                    write!(
                        formatter,
                        "put new {} to storage",
                        event_metadata.deploy().id()
                    )
                } else {
                    write!(
                        formatter,
                        "had already stored {}",
                        event_metadata.deploy().id()
                    )
                }
            }

            Event::InvalidDeployResult {
                event_metadata,
                error,
                ..
            } => {
                write!(
                    formatter,
                    "deploy received by {} with hash: {} is invalid due to error: {:?}.",
                    event_metadata.source(),
                    event_metadata.deploy().id(),
                    error
                )
            }
            Event::GetBlockResult { event_metadata, .. } => {
                write!(
                    formatter,
                    "received highest block from storage to validate deploy with hash: {}.",
                    event_metadata.deploy().id()
                )
            }
            Event::GetAccountResult { event_metadata, .. } => {
                write!(
                    formatter,
                    "verifying account to validate deploy with hash {}.",
                    event_metadata.deploy().id()
                )
            }
            Event::GetBalanceResult { event_metadata, .. } => {
                write!(
                    formatter,
                    "verifying account balance to validate deploy with hash {}.",
                    event_metadata.deploy().id()
                )
            }
            Event::VerifyPaymentLogic {
                event_metadata,
                prestate_hash,
                ..
            } => {
                write!(
                    formatter,
                    "verifying payment logic to validate deploy with hash {} with state hash: {}.",
                    event_metadata.deploy().id(),
                    prestate_hash
                )
            }
            Event::VerifySessionLogic {
                event_metadata,
                prestate_hash,
                ..
            } => {
                write!(
                    formatter,
                    "verifying session logic to validate deploy with hash {} with state hash: {}.",
                    event_metadata.deploy().id(),
                    prestate_hash
                )
            }
            Event::GetContractResult {
                event_metadata,
                prestate_hash,
                ..
            } => {
                write!(
                    formatter,
                    "verifying contract to validate deploy with hash {} with state hash: {}.",
                    event_metadata.deploy().id(),
                    prestate_hash
                )
            }
            Event::GetContractPackageResult {
                event_metadata,
                prestate_hash,
                ..
            } => {
                write!(
                    formatter,
                    "verifying contract package to validate deploy with hash {} with state hash: {}.",
                    event_metadata.deploy().id(),
                    prestate_hash
                )
            }
            Event::VerifyDeployCryptographicValidity { event_metadata, .. } => {
                write!(
                    formatter,
                    "verifying deploy cryptographic validity for deploy with hash {}.",
                    event_metadata.deploy().id(),
                )
            }
        }
    }
}
