use crate::{
    global_state::{error::Error as GlobalStateReader, state::StateReader},
    tracking_copy::{TrackingCopyEntityExt, TrackingCopyError},
    AddressGenerator, TrackingCopy,
};
use casper_types::{
    account::AccountHash, addressable_entity::NamedKeys, AddressableEntity, Chainspec,
    ContextAccessRights, FeeHandling, Key, Phase, ProtocolVersion, PublicKey, RefundHandling,
    StoredValue, TransactionHash, TransferAddr, URef, U512,
};
use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Config {
    transfer_config: TransferConfig,
    fee_handling: FeeHandling,
    refund_handling: RefundHandling,
    vesting_schedule_period_millis: u64,
    allow_auction_bids: bool,
    compute_rewards: bool,
    max_delegators_per_validator: u32,
    minimum_delegation_amount: u64,
}

impl Config {
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        transfer_config: TransferConfig,
        fee_handling: FeeHandling,
        refund_handling: RefundHandling,
        vesting_schedule_period_millis: u64,
        allow_auction_bids: bool,
        compute_rewards: bool,
        max_delegators_per_validator: u32,
        minimum_delegation_amount: u64,
    ) -> Self {
        Config {
            transfer_config,
            fee_handling,
            refund_handling,
            vesting_schedule_period_millis,
            allow_auction_bids,
            compute_rewards,
            max_delegators_per_validator,
            minimum_delegation_amount,
        }
    }

    pub fn from_chainspec(chainspec: &Chainspec) -> Self {
        let transfer_config = TransferConfig::from_chainspec(chainspec);
        let fee_handling = chainspec.core_config.fee_handling;
        let refund_handling = chainspec.core_config.refund_handling;
        let vesting_schedule_period_millis = chainspec.core_config.vesting_schedule_period.millis();
        let allow_auction_bids = chainspec.core_config.allow_auction_bids;
        let compute_rewards = chainspec.core_config.compute_rewards;
        let max_delegators_per_validator = chainspec.core_config.max_delegators_per_validator;
        let minimum_delegation_amount = chainspec.core_config.minimum_delegation_amount;
        Config::new(
            transfer_config,
            fee_handling,
            refund_handling,
            vesting_schedule_period_millis,
            allow_auction_bids,
            compute_rewards,
            max_delegators_per_validator,
            minimum_delegation_amount,
        )
    }

    pub fn transfer_config(&self) -> &TransferConfig {
        &self.transfer_config
    }

    pub fn fee_handling(&self) -> &FeeHandling {
        &self.fee_handling
    }

    pub fn refund_handling(&self) -> &RefundHandling {
        &self.refund_handling
    }

    pub fn vesting_schedule_period_millis(&self) -> u64 {
        self.vesting_schedule_period_millis
    }

    pub fn allow_auction_bids(&self) -> bool {
        self.allow_auction_bids
    }

    pub fn compute_rewards(&self) -> bool {
        self.compute_rewards
    }

    pub fn max_delegators_per_validator(&self) -> u32 {
        self.max_delegators_per_validator
    }

    pub fn minimum_delegation_amount(&self) -> u64 {
        self.minimum_delegation_amount
    }

    pub fn set_transfer_config(self, transfer_config: TransferConfig) -> Self {
        Config {
            transfer_config,
            fee_handling: self.fee_handling,
            refund_handling: self.refund_handling,
            vesting_schedule_period_millis: self.vesting_schedule_period_millis,
            max_delegators_per_validator: self.max_delegators_per_validator,
            allow_auction_bids: self.allow_auction_bids,
            minimum_delegation_amount: self.minimum_delegation_amount,
            compute_rewards: self.compute_rewards,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferConfig {
    Administered {
        administrative_accounts: BTreeSet<AccountHash>,
        allow_unrestricted_transfers: bool,
    },
    Unadministered,
}

impl TransferConfig {
    /// Returns a new instance.
    pub fn new(
        administrative_accounts: BTreeSet<AccountHash>,
        allow_unrestricted_transfers: bool,
    ) -> Self {
        if administrative_accounts.is_empty() && allow_unrestricted_transfers {
            TransferConfig::Unadministered
        } else {
            TransferConfig::Administered {
                administrative_accounts,
                allow_unrestricted_transfers,
            }
        }
    }

    /// New instance from chainspec.
    pub fn from_chainspec(chainspec: &Chainspec) -> Self {
        let administrative_accounts: BTreeSet<AccountHash> = chainspec
            .core_config
            .administrators
            .iter()
            .map(|x| x.to_account_hash())
            .collect();
        let allow_unrestricted_transfers = chainspec.core_config.allow_unrestricted_transfers;
        if administrative_accounts.is_empty() && allow_unrestricted_transfers {
            TransferConfig::Unadministered
        } else {
            TransferConfig::Administered {
                administrative_accounts,
                allow_unrestricted_transfers,
            }
        }
    }

    /// Does account hash belong to an administrative account?
    pub fn is_administrator(&self, account_hash: &AccountHash) -> bool {
        match self {
            TransferConfig::Administered {
                administrative_accounts,
                ..
            } => administrative_accounts.contains(account_hash),
            TransferConfig::Unadministered => false,
        }
    }

    /// Administrative accounts, if any.
    pub fn administrative_accounts(&self) -> BTreeSet<AccountHash> {
        match self {
            TransferConfig::Administered {
                administrative_accounts,
                ..
            } => administrative_accounts.clone(),
            TransferConfig::Unadministered => BTreeSet::default(),
        }
    }

    /// Allow unrestricted transfers.
    pub fn allow_unrestricted_transfers(&self) -> bool {
        match self {
            TransferConfig::Administered {
                allow_unrestricted_transfers,
                ..
            } => *allow_unrestricted_transfers,
            TransferConfig::Unadministered => true,
        }
    }

    /// Restricted transfer should be enforced.
    pub fn enforce_transfer_restrictions(&self, account_hash: &AccountHash) -> bool {
        !self.allow_unrestricted_transfers() && !self.is_administrator(account_hash)
    }
}

impl Default for TransferConfig {
    fn default() -> Self {
        TransferConfig::Unadministered
    }
}

pub enum Id {
    Transaction(TransactionHash),
    Seed(Vec<u8>),
}

impl Id {
    pub fn seed(&self) -> Vec<u8> {
        match self {
            Id::Transaction(hash) => hash.digest().into_vec(),
            Id::Seed(bytes) => bytes.clone(),
        }
    }
}

pub struct RuntimeNative<S> {
    config: Config,
    id: Id,

    address_generator: AddressGenerator,
    protocol_version: ProtocolVersion,

    tracking_copy: Rc<RefCell<TrackingCopy<S>>>,
    address: AccountHash,
    addressable_entity: AddressableEntity,
    named_keys: NamedKeys,
    access_rights: ContextAccessRights,
    remaining_spending_limit: U512,
    transfers: Vec<TransferAddr>,
    phase: Phase,
}

impl<S> RuntimeNative<S>
where
    S: StateReader<Key, StoredValue, Error = GlobalStateReader>,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        protocol_version: ProtocolVersion,
        config: Config,
        id: Id,
        tracking_copy: Rc<RefCell<TrackingCopy<S>>>,
        address: AccountHash,
        addressable_entity: AddressableEntity,
        named_keys: NamedKeys,
        access_rights: ContextAccessRights,
        remaining_spending_limit: U512,
        phase: Phase,
    ) -> Self {
        let seed = id.seed();
        let address_generator = AddressGenerator::new(&seed, phase);
        let transfers = vec![];
        RuntimeNative {
            config,
            id,
            address_generator,
            protocol_version,

            tracking_copy,
            address,
            addressable_entity,
            named_keys,
            access_rights,
            remaining_spending_limit,
            transfers,
            phase,
        }
    }

    /// Creates a runtime with elevated permissions for systemic behaviors.
    #[allow(clippy::too_many_arguments)]
    pub fn new_system_runtime(
        config: Config,
        protocol_version: ProtocolVersion,
        id: Id,
        tracking_copy: Rc<RefCell<TrackingCopy<S>>>,
        phase: Phase,
    ) -> Result<Self, TrackingCopyError> {
        let seed = id.seed();
        let address_generator = AddressGenerator::new(&seed, phase);
        let transfers = vec![];
        let (addressable_entity, named_keys, access_rights) =
            tracking_copy.borrow_mut().system_entity(protocol_version)?;
        let address = PublicKey::System.to_account_hash();
        let remaining_spending_limit = U512::MAX; // system has no spending limit
        Ok(RuntimeNative {
            config,
            id,
            address_generator,
            protocol_version,

            tracking_copy,
            address,
            addressable_entity,
            named_keys,
            access_rights,
            remaining_spending_limit,
            transfers,
            phase,
        })
    }

    pub fn address_generator(&mut self) -> &mut AddressGenerator {
        &mut self.address_generator
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn transfer_config(&self) -> &TransferConfig {
        &self.config.transfer_config
    }

    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }

    pub fn tracking_copy(&self) -> Rc<RefCell<TrackingCopy<S>>> {
        Rc::clone(&self.tracking_copy)
    }

    pub fn address(&self) -> AccountHash {
        self.address
    }

    pub fn addressable_entity(&self) -> &AddressableEntity {
        &self.addressable_entity
    }

    pub fn named_keys(&self) -> &NamedKeys {
        &self.named_keys
    }

    pub fn access_rights(&self) -> &ContextAccessRights {
        &self.access_rights
    }

    pub fn extend_access_rights(&mut self, urefs: &[URef]) {
        self.access_rights.extend(urefs)
    }

    pub fn remaining_spending_limit(&self) -> U512 {
        self.remaining_spending_limit
    }

    pub fn set_remaining_spending_limit(&mut self, remaining: U512) {
        self.remaining_spending_limit = remaining;
    }

    pub fn transfers(&self) -> &Vec<TransferAddr> {
        &self.transfers
    }

    pub fn push_transfer(&mut self, transfer_addr: TransferAddr) {
        self.transfers.push(transfer_addr);
    }

    pub fn id(&self) -> &Id {
        &self.id
    }

    pub fn maybe_transaction_hash(&self) -> Option<TransactionHash> {
        match self.id {
            Id::Transaction(ret) => Some(ret),
            Id::Seed(_) => None,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn vesting_schedule_period_millis(&self) -> u64 {
        self.config.vesting_schedule_period_millis
    }

    pub fn allow_auction_bids(&self) -> bool {
        self.config.allow_auction_bids
    }

    pub fn compute_rewards(&self) -> bool {
        self.config.compute_rewards
    }

    pub fn into_transfers(self) -> Vec<TransferAddr> {
        self.transfers
    }
}
