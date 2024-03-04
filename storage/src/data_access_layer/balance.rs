//! Types for balance queries.
use casper_types::{
    account::AccountHash, global_state::TrieMerkleProof, Digest, EntityAddr, Key, ProtocolVersion,
    PublicKey, StoredValue, URef, URefAddr, U512,
};

use crate::tracking_copy::TrackingCopyError;

/// Represents a way to make a balance inquiry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BalanceIdentifier {
    Purse(URef),
    Public(PublicKey),
    Account(AccountHash),
    Entity(EntityAddr),
    Internal(URefAddr),
}

impl BalanceIdentifier {
    pub fn as_key(&self) -> Key {
        match self {
            BalanceIdentifier::Purse(uref) => Key::URef(*uref),
            BalanceIdentifier::Public(public_key) => Key::Account(public_key.to_account_hash()),
            BalanceIdentifier::Account(account_hash) => Key::Account(*account_hash),
            BalanceIdentifier::Entity(entity_addr) => Key::AddressableEntity(*entity_addr),
            BalanceIdentifier::Internal(addr) => Key::Balance(*addr),
        }
    }
}

/// Represents a balance request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BalanceRequest {
    state_hash: Digest,
    protocol_version: ProtocolVersion,
    identifier: BalanceIdentifier,
}

impl BalanceRequest {
    /// Creates a new [`BalanceRequest`].
    pub fn new(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        identifier: BalanceIdentifier,
    ) -> Self {
        BalanceRequest {
            state_hash,
            protocol_version,
            identifier,
        }
    }

    /// Creates a new [`BalanceRequest`].
    pub fn from_purse(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        purse_uref: URef,
    ) -> Self {
        BalanceRequest {
            state_hash,
            protocol_version,
            identifier: BalanceIdentifier::Purse(purse_uref),
        }
    }

    /// Creates a new [`BalanceRequest`].
    pub fn from_public_key(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        public_key: PublicKey,
    ) -> Self {
        BalanceRequest {
            state_hash,
            protocol_version,
            identifier: BalanceIdentifier::Public(public_key),
        }
    }

    /// Creates a new [`BalanceRequest`].
    pub fn from_account_hash(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        account_hash: AccountHash,
    ) -> Self {
        BalanceRequest {
            state_hash,
            protocol_version,
            identifier: BalanceIdentifier::Account(account_hash),
        }
    }

    /// Creates a new [`BalanceRequest`].
    pub fn from_entity_addr(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        entity_addr: EntityAddr,
    ) -> Self {
        BalanceRequest {
            state_hash,
            protocol_version,
            identifier: BalanceIdentifier::Entity(entity_addr),
        }
    }

    /// Creates a new [`BalanceRequest`].
    pub fn from_internal(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        balance_addr: URefAddr,
    ) -> Self {
        BalanceRequest {
            state_hash,
            protocol_version,
            identifier: BalanceIdentifier::Internal(balance_addr),
        }
    }

    /// Returns a state hash.
    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    /// Protocol version.
    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }

    /// Returns the identifier [`BalanceIdentifier`].
    pub fn identifier(&self) -> &BalanceIdentifier {
        &self.identifier
    }
}

/// Result enum that represents all possible outcomes of a balance request.
#[derive(Debug)]
pub enum BalanceResult {
    /// Returned if a passed state root hash is not found.
    RootNotFound,
    /// A query returned a balance.
    Success {
        /// Purse balance.
        motes: U512,
        /// A proof that the given value is present in the Merkle trie.
        proof: Box<TrieMerkleProof<Key, StoredValue>>,
    },
    Failure(TrackingCopyError),
}

impl BalanceResult {
    /// Returns the amount of motes for a [`BalanceResult::Success`] variant.
    pub fn motes(&self) -> Option<&U512> {
        match self {
            BalanceResult::Success { motes, .. } => Some(motes),
            _ => None,
        }
    }

    /// Returns the Merkle proof for a given [`BalanceResult::Success`] variant.
    pub fn proof(self) -> Option<TrieMerkleProof<Key, StoredValue>> {
        match self {
            BalanceResult::Success { proof, .. } => Some(*proof),
            _ => None,
        }
    }
}
