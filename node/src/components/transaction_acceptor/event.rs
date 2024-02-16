use std::fmt::{self, Display, Formatter};

use serde::Serialize;

use casper_types::{
    AddressableEntity, AddressableEntityHash, BlockHeader, EntityVersion, Package, PackageHash,
    Timestamp, Transaction, U512,
};

use super::{Error, Source};
use crate::effect::Responder;

/// A utility struct to hold duplicated information across events.
#[derive(Debug, Serialize)]
pub(crate) struct EventMetadata {
    pub(crate) transaction: Transaction,
    pub(crate) source: Source,
    pub(crate) maybe_responder: Option<Responder<Result<(), Error>>>,
    pub(crate) verification_start_timestamp: Timestamp,
}

impl EventMetadata {
    pub(crate) fn new(
        transaction: Transaction,
        source: Source,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    ) -> Self {
        EventMetadata {
            transaction,
            source,
            maybe_responder,
            verification_start_timestamp: Timestamp::now(),
        }
    }
}

/// `TransactionAcceptor` events.
#[derive(Debug, Serialize)]
pub(crate) enum Event {
    /// The initiating event to accept a new `Transaction`.
    Accept {
        transaction: Transaction,
        source: Source,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    },
    /// The result of the `TransactionAcceptor` putting a `Transaction` to the storage
    /// component.
    PutToStorageResult {
        event_metadata: Box<EventMetadata>,
        is_new: bool,
    },
    /// The result of the `TransactionAcceptor` storing the approvals from a `Transaction`
    /// provided by a peer.
    StoredFinalizedApprovals {
        event_metadata: Box<EventMetadata>,
        is_new: bool,
    },
    /// The result of querying the highest available `BlockHeader` from the storage component.
    GetBlockHeaderResult {
        event_metadata: Box<EventMetadata>,
        maybe_block_header: Option<Box<BlockHeader>>,
    },
    /// The result of querying global state for the `AddressableEntity` associated with the
    /// `Transaction`'s execution context (previously known as the account).
    GetAddressableEntityResult {
        event_metadata: Box<EventMetadata>,
        block_header: Box<BlockHeader>,
        maybe_entity: Option<AddressableEntity>,
    },
    /// The result of querying the balance of the `AddressableEntity` associated with the
    /// `Transaction`.
    GetBalanceResult {
        event_metadata: Box<EventMetadata>,
        block_header: Box<BlockHeader>,
        maybe_balance: Option<U512>,
    },
    /// The result of querying global state for a `Contract` to verify the executable logic.
    GetContractResult {
        event_metadata: Box<EventMetadata>,
        block_header: Box<BlockHeader>,
        is_payment: bool,
        contract_hash: AddressableEntityHash,
        maybe_entity: Option<AddressableEntity>,
    },
    /// The result of querying global state for a `Package` to verify the executable logic.
    GetPackageResult {
        event_metadata: Box<EventMetadata>,
        block_header: Box<BlockHeader>,
        is_payment: bool,
        package_hash: PackageHash,
        maybe_package_version: Option<EntityVersion>,
        maybe_package: Option<Box<Package>>,
    },
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Accept {
                transaction,
                source,
                ..
            } => {
                write!(formatter, "accept {} from {}", transaction.hash(), source)
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
                        event_metadata.transaction.hash()
                    )
                } else {
                    write!(
                        formatter,
                        "had already stored {}",
                        event_metadata.transaction.hash()
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
                        event_metadata.transaction.hash()
                    )
                } else {
                    write!(
                        formatter,
                        "had already stored finalized approvals for {}",
                        event_metadata.transaction.hash()
                    )
                }
            }
            Event::GetBlockHeaderResult { event_metadata, .. } => {
                write!(
                    formatter,
                    "received highest block from storage to validate transaction with hash {}",
                    event_metadata.transaction.hash()
                )
            }
            Event::GetAddressableEntityResult { event_metadata, .. } => {
                write!(
                    formatter,
                    "verifying addressable entity to validate transaction with hash {}",
                    event_metadata.transaction.hash()
                )
            }
            Event::GetBalanceResult { event_metadata, .. } => {
                write!(
                    formatter,
                    "verifying account balance to validate transaction with hash {}",
                    event_metadata.transaction.hash()
                )
            }
            Event::GetContractResult {
                event_metadata,
                block_header,
                ..
            } => {
                write!(
                    formatter,
                    "verifying contract to validate transaction with hash {} with state hash {}",
                    event_metadata.transaction.hash(),
                    block_header.state_root_hash()
                )
            }
            Event::GetPackageResult {
                event_metadata,
                block_header,
                ..
            } => {
                write!(
                    formatter,
                    "verifying package to validate transaction with hash {} with state hash {}",
                    event_metadata.transaction.hash(),
                    block_header.state_root_hash()
                )
            }
        }
    }
}
