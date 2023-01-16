use std::{
    fmt::{self, Display, Formatter},
    sync::Arc,
};

use serde::Serialize;

use super::Source;
use crate::{
    components::deploy_acceptor::Error,
    effect::{announcements::RpcServerAnnouncement, Responder},
    types::{BlockHeader, Deploy},
};

use casper_hashing::Digest;
use casper_types::{
    account::{Account, AccountHash},
    Contract, ContractHash, ContractPackage, ContractPackageHash, ContractVersion, Timestamp, U512,
};

/// A utility struct to hold duplicated information across events.
#[derive(Debug, Serialize)]
pub(crate) struct EventMetadata {
    pub(crate) deploy: Arc<Deploy>,
    pub(crate) source: Source,
    pub(crate) maybe_responder: Option<Responder<Result<(), Error>>>,
}

impl EventMetadata {
    pub(crate) fn new(
        deploy: Arc<Deploy>,
        source: Source,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    ) -> Self {
        EventMetadata {
            deploy,
            source,
            maybe_responder,
        }
    }
}

/// `DeployAcceptor` events.
#[derive(Debug, Serialize)]
pub(crate) enum Event {
    /// The initiating event to accept a new `Deploy`.
    Accept {
        deploy: Arc<Deploy>,
        source: Source,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    },
    /// The result of the `DeployAcceptor` putting a `Deploy` to the storage component.
    PutToStorageResult {
        event_metadata: EventMetadata,
        is_new: bool,
        verification_start_timestamp: Timestamp,
    },
    /// The result of the `DeployAcceptor` storing the approvals from a `Deploy` provided by a
    /// peer.
    StoredFinalizedApprovals {
        event_metadata: EventMetadata,
        is_new: bool,
        verification_start_timestamp: Timestamp,
    },
    /// The result of querying the highest available `BlockHeader` from the storage component.
    GetBlockHeaderResult {
        event_metadata: EventMetadata,
        maybe_block_header: Box<Option<BlockHeader>>,
        verification_start_timestamp: Timestamp,
    },
    /// The result of querying global state for the `Account` associated with the `Deploy`.
    GetAccountResult {
        event_metadata: EventMetadata,
        prestate_hash: Digest,
        maybe_account: Option<Account>,
        verification_start_timestamp: Timestamp,
    },
    /// The result of querying the balance of the `Account` associated with the `Deploy`.
    GetBalanceResult {
        event_metadata: EventMetadata,
        prestate_hash: Digest,
        maybe_balance_value: Option<U512>,
        account_hash: AccountHash,
        verification_start_timestamp: Timestamp,
    },
    /// The result of querying global state for a `Contract` to verify the executable logic.
    GetContractResult {
        event_metadata: EventMetadata,
        prestate_hash: Digest,
        is_payment: bool,
        contract_hash: ContractHash,
        maybe_contract: Option<Contract>,
        verification_start_timestamp: Timestamp,
    },
    /// The result of querying global state for a `ContractPackage` to verify the executable logic.
    GetContractPackageResult {
        event_metadata: EventMetadata,
        prestate_hash: Digest,
        is_payment: bool,
        contract_package_hash: ContractPackageHash,
        maybe_package_version: Option<ContractVersion>,
        maybe_contract_package: Option<ContractPackage>,
        verification_start_timestamp: Timestamp,
    },
}

impl From<RpcServerAnnouncement> for Event {
    fn from(announcement: RpcServerAnnouncement) -> Self {
        match announcement {
            RpcServerAnnouncement::DeployReceived { deploy, responder } => Event::Accept {
                deploy,
                source: Source::Client,
                maybe_responder: responder,
            },
        }
    }
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Accept { deploy, source, .. } => {
                write!(formatter, "accept {} from {}", deploy.hash(), source)
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
                        event_metadata.deploy.hash()
                    )
                } else {
                    write!(
                        formatter,
                        "had already stored {}",
                        event_metadata.deploy.hash()
                    )
                }
            }
            Event::StoredFinalizedApprovals {
                event_metadata,
                is_new,
                ..
            } => {
                if *is_new {
                    write!(
                        formatter,
                        "put new finalized approvals {} to storage",
                        event_metadata.deploy.hash()
                    )
                } else {
                    write!(
                        formatter,
                        "had already stored finalized approvals for {}",
                        event_metadata.deploy.hash()
                    )
                }
            }
            Event::GetBlockHeaderResult { event_metadata, .. } => {
                write!(
                    formatter,
                    "received highest block from storage to validate deploy with hash: {}.",
                    event_metadata.deploy.hash()
                )
            }
            Event::GetAccountResult { event_metadata, .. } => {
                write!(
                    formatter,
                    "verifying account to validate deploy with hash {}.",
                    event_metadata.deploy.hash()
                )
            }
            Event::GetBalanceResult { event_metadata, .. } => {
                write!(
                    formatter,
                    "verifying account balance to validate deploy with hash {}.",
                    event_metadata.deploy.hash()
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
                    event_metadata.deploy.hash(),
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
                    event_metadata.deploy.hash(),
                    prestate_hash
                )
            }
        }
    }
}
