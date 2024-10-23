//! Types for balance queries.
use casper_types::{
    account::AccountHash,
    global_state::TrieMerkleProof,
    system::{
        handle_payment::{ACCUMULATION_PURSE_KEY, PAYMENT_PURSE_KEY, REFUND_PURSE_KEY},
        mint::BalanceHoldAddrTag,
        HANDLE_PAYMENT,
    },
    AccessRights, BlockTime, Digest, EntityAddr, HoldBalanceHandling, InitiatorAddr, Key,
    ProtocolVersion, PublicKey, StoredValue, TimeDiff, URef, URefAddr, U512,
};
use itertools::Itertools;
use num_rational::Ratio;
use num_traits::CheckedMul;
use std::{
    collections::{btree_map::Entry, BTreeMap},
    fmt::{Display, Formatter},
};
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
    Available,
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
    /// Use system refund purse (held by handle payment system contract).
    Refund,
    /// Use system payment purse (held by handle payment system contract).
    Payment,
    /// Use system accumulate purse (held by handle payment system contract).
    Accumulate,
    /// Use purse associated to specified uref.
    Purse(URef),
    /// Use main purse of entity derived from public key.
    Public(PublicKey),
    /// Use main purse of entity from account hash.
    Account(AccountHash),
    /// Use main purse of entity.
    Entity(EntityAddr),
    /// Use purse at Key::Purse(URefAddr).
    Internal(URefAddr),
    /// Penalized account identifier.
    PenalizedAccount(AccountHash),
    /// Penalized payment identifier.
    PenalizedPayment,
}

impl BalanceIdentifier {
    /// Returns underlying uref addr from balance identifier, if any.
    pub fn as_purse_addr(&self) -> Option<URefAddr> {
        match self {
            BalanceIdentifier::Internal(addr) => Some(*addr),
            BalanceIdentifier::Purse(uref) => Some(uref.addr()),
            BalanceIdentifier::Public(_)
            | BalanceIdentifier::Account(_)
            | BalanceIdentifier::PenalizedAccount(_)
            | BalanceIdentifier::PenalizedPayment
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
                match tc.runtime_footprint_by_account_hash(protocol_version, account_hash) {
                    Ok((_, entity)) => entity
                        .main_purse()
                        .ok_or_else(|| TrackingCopyError::Authorization)?,
                    Err(tce) => return Err(tce),
                }
            }
            BalanceIdentifier::Account(account_hash)
            | BalanceIdentifier::PenalizedAccount(account_hash) => {
                match tc.runtime_footprint_by_account_hash(protocol_version, *account_hash) {
                    Ok((_, entity)) => entity
                        .main_purse()
                        .ok_or_else(|| TrackingCopyError::Authorization)?,
                    Err(tce) => return Err(tce),
                }
            }
            BalanceIdentifier::Entity(entity_addr) => {
                match tc.runtime_footprint_by_entity_addr(*entity_addr) {
                    Ok(entity) => entity
                        .main_purse()
                        .ok_or_else(|| TrackingCopyError::Authorization)?,
                    Err(tce) => return Err(tce),
                }
            }
            BalanceIdentifier::Refund => {
                self.get_system_purse(tc, HANDLE_PAYMENT, REFUND_PURSE_KEY)?
            }
            BalanceIdentifier::Payment | BalanceIdentifier::PenalizedPayment => {
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

        let named_keys = tc
            .runtime_footprint_by_entity_addr(EntityAddr::System(*entity_hash))?
            .take_named_keys();

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
        matches!(
            self,
            BalanceIdentifier::Payment | BalanceIdentifier::PenalizedPayment
        )
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

/// Processing hold balance handling.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct ProcessingHoldBalanceHandling {}

impl ProcessingHoldBalanceHandling {
    /// Returns new instance.
    pub fn new() -> Self {
        ProcessingHoldBalanceHandling::default()
    }

    /// Returns handling.
    pub fn handling(&self) -> HoldBalanceHandling {
        HoldBalanceHandling::Accrued
    }

    /// Returns true if handling is amortized.
    pub fn is_amortized(&self) -> bool {
        false
    }

    /// Returns hold interval.
    pub fn interval(&self) -> TimeDiff {
        TimeDiff::default()
    }
}

impl From<(HoldBalanceHandling, u64)> for ProcessingHoldBalanceHandling {
    fn from(_value: (HoldBalanceHandling, u64)) -> Self {
        ProcessingHoldBalanceHandling::default()
    }
}

/// Gas hold balance handling.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct GasHoldBalanceHandling {
    handling: HoldBalanceHandling,
    interval: TimeDiff,
}

impl GasHoldBalanceHandling {
    /// Returns new instance.
    pub fn new(handling: HoldBalanceHandling, interval: TimeDiff) -> Self {
        GasHoldBalanceHandling { handling, interval }
    }

    /// Returns handling.
    pub fn handling(&self) -> HoldBalanceHandling {
        self.handling
    }

    /// Returns interval.
    pub fn interval(&self) -> TimeDiff {
        self.interval
    }

    /// Returns true if handling is amortized.
    pub fn is_amortized(&self) -> bool {
        matches!(self.handling, HoldBalanceHandling::Amortized)
    }
}

impl From<(HoldBalanceHandling, TimeDiff)> for GasHoldBalanceHandling {
    fn from(value: (HoldBalanceHandling, TimeDiff)) -> Self {
        GasHoldBalanceHandling {
            handling: value.0,
            interval: value.1,
        }
    }
}

impl From<(HoldBalanceHandling, u64)> for GasHoldBalanceHandling {
    fn from(value: (HoldBalanceHandling, u64)) -> Self {
        GasHoldBalanceHandling {
            handling: value.0,
            interval: TimeDiff::from_millis(value.1),
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

/// Available balance checker.
pub trait AvailableBalanceChecker {
    /// Calculate and return available balance.
    fn available_balance(
        &self,
        block_time: BlockTime,
        total_balance: U512,
        gas_hold_balance_handling: GasHoldBalanceHandling,
        processing_hold_balance_handling: ProcessingHoldBalanceHandling,
    ) -> Result<U512, BalanceFailure> {
        if self.is_empty() {
            return Ok(total_balance);
        }

        let gas_held = match gas_hold_balance_handling.handling() {
            HoldBalanceHandling::Accrued => self.accrued(BalanceHoldAddrTag::Gas),
            HoldBalanceHandling::Amortized => {
                let interval = gas_hold_balance_handling.interval();
                self.amortization(BalanceHoldAddrTag::Gas, block_time, interval)?
            }
        };

        let processing_held = match processing_hold_balance_handling.handling() {
            HoldBalanceHandling::Accrued => self.accrued(BalanceHoldAddrTag::Processing),
            HoldBalanceHandling::Amortized => {
                let interval = processing_hold_balance_handling.interval();
                self.amortization(BalanceHoldAddrTag::Processing, block_time, interval)?
            }
        };

        let held = gas_held.saturating_add(processing_held);

        debug_assert!(
            total_balance >= held,
            "it should not be possible to hold more than the total available"
        );
        match total_balance.checked_sub(held) {
            Some(available_balance) => Ok(available_balance),
            None => {
                error!(%held, %total_balance, "held amount exceeds total balance, which should never occur.");
                Err(BalanceFailure::HeldExceedsTotal)
            }
        }
    }

    /// Calculates amortization.
    fn amortization(
        &self,
        hold_kind: BalanceHoldAddrTag,
        block_time: BlockTime,
        interval: TimeDiff,
    ) -> Result<U512, BalanceFailure> {
        let mut held = U512::zero();
        let block_time = block_time.value();
        let interval = interval.millis();

        for (hold_created_time, holds) in self.holds(hold_kind) {
            let hold_created_time = hold_created_time.value();
            if hold_created_time > block_time {
                continue;
            }
            let expiry = hold_created_time.saturating_add(interval);
            if block_time > expiry {
                continue;
            }
            // total held amount
            let held_ratio = Ratio::new_raw(
                holds.values().copied().collect_vec().into_iter().sum(),
                U512::one(),
            );
            // remaining time
            let remaining_time = U512::from(expiry.saturating_sub(block_time));
            // remaining time over total time
            let ratio = Ratio::new_raw(remaining_time, U512::from(interval));
            /*
                EXAMPLE: 1000 held for 24 hours
                if 1 hours has elapsed, held amount = 1000 * (23/24) == 958
                if 2 hours has elapsed, held amount = 1000 * (22/24) == 916
                ...
                if 23 hours has elapsed, held amount    = 1000 * (1/24) == 41
                if 23.50 hours has elapsed, held amount = 1000 * (1/48) == 20
                if 23.75 hours has elapsed, held amount = 1000 * (1/96) == 10
                                                (54000 ms / 5184000 ms)
            */
            match held_ratio.checked_mul(&ratio) {
                Some(amortized) => held += amortized.to_integer(),
                None => return Err(BalanceFailure::AmortizationFailure),
            }
        }
        Ok(held)
    }

    /// Return accrued amount.
    fn accrued(&self, hold_kind: BalanceHoldAddrTag) -> U512;

    /// Return holds.
    fn holds(&self, hold_kind: BalanceHoldAddrTag) -> BTreeMap<BlockTime, BalanceHolds>;

    /// Return true if empty.
    fn is_empty(&self) -> bool;
}

/// Balance holds with Merkle proofs.
pub type BalanceHolds = BTreeMap<BalanceHoldAddrTag, U512>;

impl AvailableBalanceChecker for BTreeMap<BlockTime, BalanceHolds> {
    fn accrued(&self, hold_kind: BalanceHoldAddrTag) -> U512 {
        self.values()
            .filter_map(|holds| holds.get(&hold_kind).copied())
            .collect_vec()
            .into_iter()
            .sum()
    }

    fn holds(&self, hold_kind: BalanceHoldAddrTag) -> BTreeMap<BlockTime, BalanceHolds> {
        let mut ret = BTreeMap::new();
        for (k, v) in self {
            if let Some(hold) = v.get(&hold_kind) {
                let mut inner = BTreeMap::new();
                inner.insert(hold_kind, *hold);
                ret.insert(*k, inner);
            }
        }
        ret
    }

    fn is_empty(&self) -> bool {
        self.is_empty()
    }
}

/// Balance holds with Merkle proofs.
pub type BalanceHoldsWithProof =
    BTreeMap<BalanceHoldAddrTag, (U512, TrieMerkleProof<Key, StoredValue>)>;

impl AvailableBalanceChecker for BTreeMap<BlockTime, BalanceHoldsWithProof> {
    fn accrued(&self, hold_kind: BalanceHoldAddrTag) -> U512 {
        self.values()
            .filter_map(|holds| holds.get(&hold_kind))
            .map(|(amount, _)| *amount)
            .collect_vec()
            .into_iter()
            .sum()
    }

    fn holds(&self, hold_kind: BalanceHoldAddrTag) -> BTreeMap<BlockTime, BalanceHolds> {
        let mut ret: BTreeMap<BlockTime, BalanceHolds> = BTreeMap::new();
        for (block_time, holds_with_proof) in self {
            let mut holds: BTreeMap<BalanceHoldAddrTag, U512> = BTreeMap::new();
            for (addr, (held, _)) in holds_with_proof {
                if addr == &hold_kind {
                    match holds.entry(*addr) {
                        Entry::Vacant(v) => v.insert(*held),
                        Entry::Occupied(mut o) => &mut o.insert(*held),
                    };
                }
            }
            if !holds.is_empty() {
                match ret.entry(*block_time) {
                    Entry::Vacant(v) => v.insert(holds),
                    Entry::Occupied(mut o) => &mut o.insert(holds),
                };
            }
        }
        ret
    }

    fn is_empty(&self) -> bool {
        self.is_empty()
    }
}

/// Proofs result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProofsResult {
    /// Not requested.
    NotRequested {
        /// Any time-relevant active holds on the balance, without proofs.
        balance_holds: BTreeMap<BlockTime, BalanceHolds>,
    },
    /// Proofs.
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

    /// Returns the available balance, calculated using imputed values.
    #[allow(clippy::result_unit_err)]
    pub fn available_balance(
        &self,
        block_time: BlockTime,
        total_balance: U512,
        gas_hold_balance_handling: GasHoldBalanceHandling,
        processing_hold_balance_handling: ProcessingHoldBalanceHandling,
    ) -> Result<U512, BalanceFailure> {
        match self {
            ProofsResult::NotRequested { balance_holds } => balance_holds.available_balance(
                block_time,
                total_balance,
                gas_hold_balance_handling,
                processing_hold_balance_handling,
            ),
            ProofsResult::Proofs { balance_holds, .. } => balance_holds.available_balance(
                block_time,
                total_balance,
                gas_hold_balance_handling,
                processing_hold_balance_handling,
            ),
        }
    }
}

/// Balance failure.
#[derive(Debug, Clone)]
pub enum BalanceFailure {
    /// Failed to calculate amortization (checked multiplication).
    AmortizationFailure,
    /// Held amount exceeds total balance, which should never occur.
    HeldExceedsTotal,
}

impl Display for BalanceFailure {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            BalanceFailure::AmortizationFailure => {
                write!(
                    f,
                    "AmortizationFailure: failed to calculate amortization (checked multiplication)."
                )
            }
            BalanceFailure::HeldExceedsTotal => {
                write!(
                    f,
                    "HeldExceedsTotal: held amount exceeds total balance, which should never occur."
                )
            }
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
    /// Failure.
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

    /// Tracking copy error, if any.
    pub fn error(&self) -> Option<&TrackingCopyError> {
        match self {
            BalanceResult::RootNotFound | BalanceResult::Success { .. } => None,
            BalanceResult::Failure(err) => Some(err),
        }
    }
}

impl From<TrackingCopyError> for BalanceResult {
    fn from(tce: TrackingCopyError) -> Self {
        BalanceResult::Failure(tce)
    }
}
