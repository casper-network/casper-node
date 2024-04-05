//! Types for balance queries.
use casper_types::{
    account::AccountHash,
    global_state::TrieMerkleProof,
    system::{
        handle_payment::{ACCUMULATION_PURSE_KEY, PAYMENT_PURSE_KEY, REFUND_PURSE_KEY},
        mint::BalanceHoldAddrTag,
        HANDLE_PAYMENT,
    },
    AccessRights, BlockTime, Digest, EntityAddr, HoldsEpoch, InitiatorAddr, Key, ProtocolVersion,
    PublicKey, StoredValue, URef, URefAddr, U512,
};
use itertools::Itertools;
use std::collections::BTreeMap;
use tracing::error;

use crate::{
    global_state::state::StateReader,
    tracking_copy::{TrackingCopyEntityExt, TrackingCopyError, TrackingCopyExt},
    TrackingCopy,
};

/// How to handle available balance inquiry?
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum BalanceHandling {
    /// Ignore balance holds.
    #[default]
    Total,
    /// Adjust for balance holds (if any).
    Available { holds_epoch: HoldsEpoch },
}

/// Merkle proof handling options.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum ProofHandling {
    /// Do not attempt to provide proofs.
    #[default]
    NoProofs,
    /// Provide proofs.
    Proofs,
}

/// Represents a way to make a balance inquiry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BalanceIdentifier {
    Refund,
    Payment,
    Accumulate,
    Purse(URef),
    Public(PublicKey),
    Account(AccountHash),
    Entity(EntityAddr),
    Internal(URefAddr),
    PenalizedAccount(AccountHash),
}

impl BalanceIdentifier {
    pub fn as_purse_addr(&self) -> Option<URefAddr> {
        match self {
            BalanceIdentifier::Internal(addr) => Some(*addr),
            BalanceIdentifier::Purse(uref) => Some(uref.addr()),
            BalanceIdentifier::Public(_)
            | BalanceIdentifier::Account(_)
            | BalanceIdentifier::PenalizedAccount(_)
            | BalanceIdentifier::Entity(_)
            | BalanceIdentifier::Refund
            | BalanceIdentifier::Payment
            | BalanceIdentifier::Accumulate => None,
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
            BalanceIdentifier::Internal(addr) => URef::new(*addr, AccessRights::READ),
            BalanceIdentifier::Purse(purse_uref) => *purse_uref,
            BalanceIdentifier::Public(public_key) => {
                let account_hash = public_key.to_account_hash();
                match tc.get_addressable_entity_by_account_hash(protocol_version, account_hash) {
                    Ok(entity) => entity.main_purse(),
                    Err(tce) => return Err(tce),
                }
            }
            BalanceIdentifier::Account(account_hash)
            | BalanceIdentifier::PenalizedAccount(account_hash) => {
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
            BalanceIdentifier::Refund => {
                self.get_system_purse(tc, HANDLE_PAYMENT, REFUND_PURSE_KEY)?
            }
            BalanceIdentifier::Payment => {
                self.get_system_purse(tc, HANDLE_PAYMENT, PAYMENT_PURSE_KEY)?
            }
            BalanceIdentifier::Accumulate => {
                self.get_system_purse(tc, HANDLE_PAYMENT, ACCUMULATION_PURSE_KEY)?
            }
        };
        Ok(purse_uref)
    }

    fn get_system_purse<S>(
        &self,
        tc: &mut TrackingCopy<S>,
        system_contract_name: &str,
        named_key_name: &str,
    ) -> Result<URef, TrackingCopyError>
    where
        S: StateReader<Key, StoredValue, Error = crate::global_state::error::Error>,
    {
        let system_contract_registry = tc.get_system_entity_registry()?;

        let entity_hash = system_contract_registry
            .get(system_contract_name)
            .ok_or_else(|| {
                error!("Missing system handle payment contract hash");
                TrackingCopyError::MissingSystemContractHash(system_contract_name.to_string())
            })?;

        let named_keys = tc.get_named_keys(EntityAddr::System(entity_hash.value()))?;
        let named_key =
            named_keys
                .get(named_key_name)
                .ok_or(TrackingCopyError::NamedKeyNotFound(
                    named_key_name.to_string(),
                ))?;
        let uref = named_key
            .as_uref()
            .ok_or(TrackingCopyError::UnexpectedKeyVariant(*named_key))?;
        Ok(*uref)
    }

    /// Is this balance identifier for penalty?
    pub fn is_penalty(&self) -> bool {
        // currently there is one variant of this kind, but more may be added later to
        // support more use cases.
        matches!(self, BalanceIdentifier::PenalizedAccount(_))
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
    proof_handling: ProofHandling,
}

impl BalanceRequest {
    /// Creates a new [`BalanceRequest`].
    pub fn new(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        identifier: BalanceIdentifier,
        balance_handling: BalanceHandling,
        proof_handling: ProofHandling,
    ) -> Self {
        BalanceRequest {
            state_hash,
            protocol_version,
            identifier,
            balance_handling,
            proof_handling,
        }
    }

    /// Creates a new [`BalanceRequest`].
    pub fn from_purse(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        purse_uref: URef,
        balance_handling: BalanceHandling,
        proof_handling: ProofHandling,
    ) -> Self {
        BalanceRequest {
            state_hash,
            protocol_version,
            identifier: BalanceIdentifier::Purse(purse_uref),
            balance_handling,
            proof_handling,
        }
    }

    /// Creates a new [`BalanceRequest`].
    pub fn from_public_key(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        public_key: PublicKey,
        balance_handling: BalanceHandling,
        proof_handling: ProofHandling,
    ) -> Self {
        BalanceRequest {
            state_hash,
            protocol_version,
            identifier: BalanceIdentifier::Public(public_key),
            balance_handling,
            proof_handling,
        }
    }

    /// Creates a new [`BalanceRequest`].
    pub fn from_account_hash(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        account_hash: AccountHash,
        balance_handling: BalanceHandling,
        proof_handling: ProofHandling,
    ) -> Self {
        BalanceRequest {
            state_hash,
            protocol_version,
            identifier: BalanceIdentifier::Account(account_hash),
            balance_handling,
            proof_handling,
        }
    }

    /// Creates a new [`BalanceRequest`].
    pub fn from_entity_addr(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        entity_addr: EntityAddr,
        balance_handling: BalanceHandling,
        proof_handling: ProofHandling,
    ) -> Self {
        BalanceRequest {
            state_hash,
            protocol_version,
            identifier: BalanceIdentifier::Entity(entity_addr),
            balance_handling,
            proof_handling,
        }
    }

    /// Creates a new [`BalanceRequest`].
    pub fn from_internal(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        balance_addr: URefAddr,
        balance_handling: BalanceHandling,
        proof_handling: ProofHandling,
    ) -> Self {
        BalanceRequest {
            state_hash,
            protocol_version,
            identifier: BalanceIdentifier::Internal(balance_addr),
            balance_handling,
            proof_handling,
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

    /// Returns proof handling.
    pub fn proof_handling(&self) -> ProofHandling {
        self.proof_handling
    }
}

/// Balance holds with Merkle proofs.
pub type BalanceHolds = BTreeMap<BalanceHoldAddrTag, U512>;

/// Balance holds with Merkle proofs.
pub type BalanceHoldsWithProof =
    BTreeMap<BalanceHoldAddrTag, (U512, TrieMerkleProof<Key, StoredValue>)>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProofsResult {
    NotRequested {
        /// Any time-relevant active holds on the balance, without proofs.
        balance_holds: BTreeMap<BlockTime, BalanceHolds>,
    },
    Proofs {
        /// A proof that the given value is present in the Merkle trie.
        total_balance_proof: Box<TrieMerkleProof<Key, StoredValue>>,
        /// Any time-relevant active holds on the balance, with proofs..
        balance_holds: BTreeMap<BlockTime, BalanceHoldsWithProof>,
    },
}

impl ProofsResult {
    /// Returns total balance proof, if any.
    pub fn total_balance_proof(&self) -> Option<&TrieMerkleProof<Key, StoredValue>> {
        match self {
            ProofsResult::NotRequested { .. } => None,
            ProofsResult::Proofs {
                total_balance_proof,
                ..
            } => Some(total_balance_proof),
        }
    }

    /// Returns balance holds, if any.
    pub fn balance_holds_with_proof(&self) -> Option<&BTreeMap<BlockTime, BalanceHoldsWithProof>> {
        match self {
            ProofsResult::NotRequested { .. } => None,
            ProofsResult::Proofs { balance_holds, .. } => Some(balance_holds),
        }
    }

    /// Returns balance holds, if any.
    pub fn balance_holds(&self) -> Option<&BTreeMap<BlockTime, BalanceHolds>> {
        match self {
            ProofsResult::NotRequested { balance_holds } => Some(balance_holds),
            ProofsResult::Proofs { .. } => None,
        }
    }

    /// Returns the total held amount.
    pub fn total_held_amount(&self) -> U512 {
        match self {
            ProofsResult::NotRequested { balance_holds } => balance_holds
                .values()
                .flat_map(|holds| holds.values().copied())
                .collect_vec()
                .into_iter()
                .sum(),
            ProofsResult::Proofs { balance_holds, .. } => balance_holds
                .values()
                .flat_map(|holds| holds.values().map(|(v, _)| *v))
                .collect_vec()
                .into_iter()
                .sum(),
        }
    }
}

/// Result enum that represents all possible outcomes of a balance request.
#[derive(Debug, Clone)]
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
        /// Proofs result.
        proofs_result: ProofsResult,
    },
    Failure(TrackingCopyError),
}

impl BalanceResult {
    /// Returns the purse address for a [`BalanceResult::Success`] variant.
    pub fn purse_addr(&self) -> Option<URefAddr> {
        match self {
            BalanceResult::Success { purse_addr, .. } => Some(*purse_addr),
            _ => None,
        }
    }

    /// Returns the total balance for a [`BalanceResult::Success`] variant.
    pub fn total_balance(&self) -> Option<&U512> {
        match self {
            BalanceResult::Success { total_balance, .. } => Some(total_balance),
            _ => None,
        }
    }

    /// Returns the available balance for a [`BalanceResult::Success`] variant.
    pub fn available_balance(&self) -> Option<&U512> {
        match self {
            BalanceResult::Success {
                available_balance, ..
            } => Some(available_balance),
            _ => None,
        }
    }

    /// Returns the Merkle proofs, if any.
    pub fn proofs_result(self) -> Option<ProofsResult> {
        match self {
            BalanceResult::Success { proofs_result, .. } => Some(proofs_result),
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

    /// Was the balance request successful?
    pub fn is_success(&self) -> bool {
        match self {
            BalanceResult::RootNotFound | BalanceResult::Failure(_) => false,
            BalanceResult::Success { .. } => true,
        }
    }
}
