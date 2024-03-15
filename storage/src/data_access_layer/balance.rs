//! Types for balance queries.
use crate::data_access_layer::BalanceHoldRequest;
use casper_types::{
    account::AccountHash, global_state::TrieMerkleProof, system::mint::BalanceHoldAddrTag,
    AccessRights, BlockTime, Digest, EntityAddr, InitiatorAddr, Key, ProtocolVersion, PublicKey,
    StoredValue, URef, URefAddr, U512,
};
use std::collections::BTreeMap;

use crate::{
    global_state::state::StateReader,
    tracking_copy::{TrackingCopyEntityExt, TrackingCopyError},
    TrackingCopy,
};

/// How to handle available balance inquiry?
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum BalanceHandling {
    /// Ignore balance holds.
    #[default]
    Total,
    /// Adjust for balance holds (if any).
    Available { block_time: u64, hold_interval: u64 },
}

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
    /// Self as key.
    pub fn as_key(&self) -> Key {
        match self {
            BalanceIdentifier::Purse(uref) => Key::URef(*uref),
            BalanceIdentifier::Public(public_key) => Key::Account(public_key.to_account_hash()),
            BalanceIdentifier::Account(account_hash) => Key::Account(*account_hash),
            BalanceIdentifier::Entity(entity_addr) => Key::AddressableEntity(*entity_addr),
            BalanceIdentifier::Internal(addr) => Key::Balance(*addr),
        }
    }

    // #[cfg(test)]
    pub fn as_purse_addr(&self) -> Option<URefAddr> {
        match self {
            BalanceIdentifier::Purse(uref) => Some(uref.addr()),
            BalanceIdentifier::Internal(addr) => Some(*addr),
            BalanceIdentifier::Public(_)
            | BalanceIdentifier::Account(_)
            | BalanceIdentifier::Entity(_) => None,
        }
    }

    /// Return purse_uref, if able.
    pub fn purse_uref<S>(
        &self,
        tc: &mut TrackingCopy<S>,
        protocol_version: ProtocolVersion,
    ) -> Result<URef, TrackingCopyError>
    where
        S: StateReader<Key, StoredValue, Error = crate::global_state::error::Error>,
    {
        let purse_uref = match self {
            BalanceIdentifier::Purse(purse_uref) => *purse_uref,
            BalanceIdentifier::Public(public_key) => {
                let account_hash = public_key.to_account_hash();
                match tc.get_addressable_entity_by_account_hash(protocol_version, account_hash) {
                    Ok(entity) => entity.main_purse(),
                    Err(tce) => return Err(tce),
                }
            }
            BalanceIdentifier::Account(account_hash) => {
                match tc.get_addressable_entity_by_account_hash(protocol_version, *account_hash) {
                    Ok(entity) => entity.main_purse(),
                    Err(tce) => return Err(tce),
                }
            }
            BalanceIdentifier::Entity(entity_addr) => {
                match tc.get_addressable_entity(*entity_addr) {
                    Ok(entity) => entity.main_purse(),
                    Err(tce) => return Err(tce),
                }
            }
            BalanceIdentifier::Internal(addr) => URef::new(*addr, AccessRights::READ),
        };
        Ok(purse_uref)
    }
}

impl Default for BalanceIdentifier {
    fn default() -> Self {
        BalanceIdentifier::Purse(URef::default())
    }
}

impl From<InitiatorAddr> for BalanceIdentifier {
    fn from(value: InitiatorAddr) -> Self {
        match value {
            InitiatorAddr::PublicKey(public_key) => BalanceIdentifier::Public(public_key),
            InitiatorAddr::AccountHash(account_hash) => BalanceIdentifier::Account(account_hash),
        }
    }
}

/// Represents a balance request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BalanceRequest {
    state_hash: Digest,
    protocol_version: ProtocolVersion,
    identifier: BalanceIdentifier,
    balance_handling: BalanceHandling,
}

impl BalanceRequest {
    /// Creates a new [`BalanceRequest`].
    pub fn new(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        identifier: BalanceIdentifier,
        balance_handling: BalanceHandling,
    ) -> Self {
        BalanceRequest {
            state_hash,
            protocol_version,
            identifier,
            balance_handling,
        }
    }

    /// Creates a new [`BalanceRequest`].
    pub fn from_purse(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        purse_uref: URef,
        balance_handling: BalanceHandling,
    ) -> Self {
        BalanceRequest {
            state_hash,
            protocol_version,
            identifier: BalanceIdentifier::Purse(purse_uref),
            balance_handling,
        }
    }

    /// Creates a new [`BalanceRequest`].
    pub fn from_public_key(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        public_key: PublicKey,
        balance_handling: BalanceHandling,
    ) -> Self {
        BalanceRequest {
            state_hash,
            protocol_version,
            identifier: BalanceIdentifier::Public(public_key),
            balance_handling,
        }
    }

    /// Creates a new [`BalanceRequest`].
    pub fn from_account_hash(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        account_hash: AccountHash,
        balance_handling: BalanceHandling,
    ) -> Self {
        BalanceRequest {
            state_hash,
            protocol_version,
            identifier: BalanceIdentifier::Account(account_hash),
            balance_handling,
        }
    }

    /// Creates a new [`BalanceRequest`].
    pub fn from_entity_addr(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        entity_addr: EntityAddr,
        balance_handling: BalanceHandling,
    ) -> Self {
        BalanceRequest {
            state_hash,
            protocol_version,
            identifier: BalanceIdentifier::Entity(entity_addr),
            balance_handling,
        }
    }

    /// Creates a new [`BalanceRequest`].
    pub fn from_internal(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        balance_addr: URefAddr,
        balance_handling: BalanceHandling,
    ) -> Self {
        BalanceRequest {
            state_hash,
            protocol_version,
            identifier: BalanceIdentifier::Internal(balance_addr),
            balance_handling,
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

    /// Returns the block time.
    pub fn balance_handling(&self) -> BalanceHandling {
        self.balance_handling
    }
}

impl From<BalanceHoldRequest> for BalanceRequest {
    fn from(value: BalanceHoldRequest) -> Self {
        let balance_handling = BalanceHandling::Available {
            block_time: value.block_time().value(),
            hold_interval: value.hold_interval().millis(),
        };
        BalanceRequest::new(
            value.state_hash(),
            value.protocol_version(),
            value.identifier().clone(),
            balance_handling,
        )
    }
}

/// Balance holds with Merkle proofs.
pub type BalanceHoldsWithProof =
    BTreeMap<BalanceHoldAddrTag, (U512, TrieMerkleProof<Key, StoredValue>)>;

/// Result enum that represents all possible outcomes of a balance request.
#[derive(Debug)]
pub enum BalanceResult {
    /// Returned if a passed state root hash is not found.
    RootNotFound,
    /// A query returned a balance.
    Success {
        /// The purse address.
        purse_addr: URefAddr,
        /// The purses total balance, not considering holds.
        total_balance: U512,
        /// The available balance (total balance - sum of all active holds).
        available_balance: U512,
        /// A proof that the given value is present in the Merkle trie.
        total_balance_proof: Box<TrieMerkleProof<Key, StoredValue>>,
        /// Any time-relevant active holds on the balance.
        balance_holds: BTreeMap<BlockTime, BalanceHoldsWithProof>,
    },
    Failure(TrackingCopyError),
}

impl BalanceResult {
    /// Returns the amount of motes for a [`BalanceResult::Success`] variant.
    pub fn motes(&self) -> Option<&U512> {
        match self {
            BalanceResult::Success {
                available_balance: motes,
                ..
            } => Some(motes),
            _ => None,
        }
    }

    /// Returns the Merkle proof for a given [`BalanceResult::Success`] variant.
    pub fn proof(self) -> Option<TrieMerkleProof<Key, StoredValue>> {
        match self {
            BalanceResult::Success {
                total_balance_proof: proof,
                ..
            } => Some(*proof),
            _ => None,
        }
    }

    /// Is the available balance sufficient to cover the cost?
    pub fn is_sufficient(&self, cost: U512) -> bool {
        match self {
            BalanceResult::RootNotFound | BalanceResult::Failure(_) => false,
            BalanceResult::Success {
                available_balance, ..
            } => available_balance >= &cost,
        }
    }
}
