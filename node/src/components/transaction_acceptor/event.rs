use std::fmt::{self, Display, Formatter};

use serde::Serialize;

use casper_types::{
    account::AccountHash, contracts::ProtocolVersionMajor, evm, AddressableEntity,
    AddressableEntityHash, BlockHeader, EntityVersion, Package, PackageHash, Timestamp,
    Transaction, URef, U512,
};

use super::{Error, Source};
use crate::{effect::Responder, types::MetaTransaction};

/// A utility struct to hold duplicated information across events.
#[derive(Debug, Serialize)]
pub(crate) struct EventMetadata {
    pub(crate) transaction: Transaction,
    pub(crate) meta_transaction: MetaTransaction,
    pub(crate) source: Source,
    pub(crate) maybe_responder: Option<Responder<Result<(), Error>>>,
    pub(crate) verification_start_timestamp: Timestamp,
}

impl EventMetadata {
    pub(crate) fn new(
        transaction: Transaction,
        meta_transaction: MetaTransaction,
        source: Source,
        maybe_responder: Option<Responder<Result<(), Error>>>,
        verification_start_timestamp: Timestamp,
    ) -> Self {
        EventMetadata {
            transaction,
            meta_transaction,
            source,
            maybe_responder,
            verification_start_timestamp,
        }
    }
}

/// Result of looking up the identity record for an EVM address.
#[derive(Clone, Debug, Serialize)]
pub(crate) enum EvmAccountLookup {
    /// Identity pointer to a Casper account.
    Account(AccountHash),
    /// Identity pointer to an EVM-native purse.
    Purse(URef),
    /// The EVM account identity record exists but is malformed.
    Invalid(String),
    /// No EVM account identity exists yet.
    Missing,
}

/// Source used for EVM client balance checks.
#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) enum EvmBalanceSource {
    /// Check the given purse directly.
    Purse(URef),
    /// Check the main purse of a Casper account.
    Account(AccountHash),
}

/// Result of looking up a split EVM nonce record.
#[derive(Clone, Debug, Serialize)]
pub(crate) enum EvmNonceLookup {
    /// Nonce record exists and decoded successfully.
    Value(u64),
    /// No nonce record exists.
    Missing,
    /// The nonce record exists but is malformed.
    Invalid(String),
}

/// Result of looking up a split EVM code-hash record.
#[derive(Clone, Debug, Serialize)]
pub(crate) enum EvmCodeHashLookup {
    /// Code-hash record exists and decoded successfully.
    Value(evm::Hash),
    /// No code-hash record exists.
    Missing,
    /// The code-hash record exists but is malformed.
    Invalid(String),
}

/// `TransactionAcceptor` events.
#[allow(clippy::large_enum_variant)]
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
    /// The result of querying global state for the EVM account associated with an EVM transaction.
    GetEvmAccountResult {
        event_metadata: Box<EventMetadata>,
        block_header: Box<BlockHeader>,
        account: EvmAccountLookup,
    },
    /// The result of querying global state for an EVM account nonce.
    GetEvmNonceResult {
        event_metadata: Box<EventMetadata>,
        block_header: Box<BlockHeader>,
        balance_source: EvmBalanceSource,
        nonce: EvmNonceLookup,
    },
    /// The result of querying nonce for an EVM transaction whose identity pointer is missing.
    GetMissingEvmIdentityNonceResult {
        event_metadata: Box<EventMetadata>,
        block_header: Box<BlockHeader>,
        nonce: EvmNonceLookup,
    },
    /// The result of querying code hash for an EVM transaction whose identity pointer is missing.
    GetMissingEvmIdentityCodeHashResult {
        event_metadata: Box<EventMetadata>,
        block_header: Box<BlockHeader>,
        expected_nonce: u64,
        code_hash: EvmCodeHashLookup,
    },
    /// The result of querying the Casper account matching a missing EVM identity.
    GetEvmAccountEntityResult {
        event_metadata: Box<EventMetadata>,
        block_header: Box<BlockHeader>,
        expected_nonce: u64,
        account_hash: AccountHash,
        maybe_entity: Option<AddressableEntity>,
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
        maybe_entity_version: Option<EntityVersion>,
        maybe_protocol_version_major: Option<ProtocolVersionMajor>,
        maybe_package: Option<Box<Package>>,
    },
    /// The result of querying global state for an `EntryPoint` to verify the executable logic.
    GetEntryPointResult {
        event_metadata: Box<EventMetadata>,
        block_header: Box<BlockHeader>,
        is_payment: bool,
        entry_point_name: String,
        addressable_entity: AddressableEntity,
        entry_point_exists: bool,
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
            Event::GetEvmAccountResult { event_metadata, .. } => {
                write!(
                    formatter,
                    "verifying EVM account identity to validate transaction with hash {}",
                    event_metadata.transaction.hash()
                )
            }
            Event::GetEvmNonceResult { event_metadata, .. } => {
                write!(
                    formatter,
                    "verifying EVM account nonce to validate transaction with hash {}",
                    event_metadata.transaction.hash()
                )
            }
            Event::GetMissingEvmIdentityNonceResult { event_metadata, .. } => {
                write!(
                    formatter,
                    "verifying missing EVM identity nonce to validate transaction with hash {}",
                    event_metadata.transaction.hash()
                )
            }
            Event::GetMissingEvmIdentityCodeHashResult { event_metadata, .. } => {
                write!(
                    formatter,
                    "verifying missing EVM identity code hash to validate transaction with hash {}",
                    event_metadata.transaction.hash()
                )
            }
            Event::GetEvmAccountEntityResult { event_metadata, .. } => {
                write!(
                    formatter,
                    "verifying EVM signer account to validate transaction with hash {}",
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
            Event::GetEntryPointResult {
                event_metadata,
                block_header,
                ..
            } => {
                write!(
                    formatter,
                    "verifying entry point to validate transaction with hash {} with state hash {}",
                    event_metadata.transaction.hash(),
                    block_header.state_root_hash(),
                )
            }
        }
    }
}
