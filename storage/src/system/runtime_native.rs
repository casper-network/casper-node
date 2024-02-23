use crate::{
    global_state::{error::Error as GlobalStateReader, state::StateReader},
    tracking_copy::{TrackingCopyEntityExt, TrackingCopyError},
    AddressGenerator, TrackingCopy,
};
use casper_types::{
    account::AccountHash, addressable_entity::NamedKeys, AddressableEntity, ContextAccessRights,
    Key, Phase, ProtocolVersion, PublicKey, StoredValue, TransactionHash, TransferAddr, URef, U512,
};
use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Config {
    transfer_config: TransferConfig,
    vesting_schedule_period_millis: u64,
    allow_auction_bids: bool,
    should_compute_rewards: bool,
    max_delegators_per_validator: Option<u32>,
    minimum_delegation_amount: u64,
}

impl Config {
    pub fn new(
        transfer_config: TransferConfig,
        vesting_schedule_period_millis: u64,
        allow_auction_bids: bool,
        should_compute_rewards: bool,
        max_delegators_per_validator: Option<u32>,
        minimum_delegation_amount: u64,
    ) -> Self {
        Config {
            transfer_config,
            vesting_schedule_period_millis,
            allow_auction_bids,
            should_compute_rewards,
            max_delegators_per_validator,
            minimum_delegation_amount,
        }
    }

    pub fn for_transfer(transfer_config: TransferConfig) -> Self {
        Config {
            transfer_config,
            vesting_schedule_period_millis: 0,
            allow_auction_bids: false,
            should_compute_rewards: false,
            max_delegators_per_validator: None,
            minimum_delegation_amount: 0,
        }
    }

    pub fn transfer_config(&self) -> &TransferConfig {
        &self.transfer_config
    }
    pub fn vesting_schedule_period_millis(&self) -> u64 {
        self.vesting_schedule_period_millis
    }
    pub fn allow_auction_bids(&self) -> bool {
        self.allow_auction_bids
    }
    pub fn should_compute_rewards(&self) -> bool {
        self.should_compute_rewards
    }

    pub fn max_delegators_per_validator(&self) -> Option<u32> {
        self.max_delegators_per_validator
    }

    pub fn minimum_delegation_amount(&self) -> u64 {
        self.minimum_delegation_amount
    }

    pub fn set_transfer_config(self, transfer_config: TransferConfig) -> Self {
        Config {
            transfer_config,
            vesting_schedule_period_millis: self.vesting_schedule_period_millis,
            max_delegators_per_validator: self.max_delegators_per_validator,
            allow_auction_bids: self.allow_auction_bids,
            minimum_delegation_amount: self.minimum_delegation_amount,
            should_compute_rewards: self.should_compute_rewards,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferConfig {
    Administered {
        administrative_accounts: BTreeSet<AccountHash>,
        allow_unrestricted_transfer: bool,
    },
    Unadministered,
}

impl TransferConfig {
    /// Returns a new instance.
    pub fn new(
        administrative_accounts: BTreeSet<AccountHash>,
        allow_unrestricted_transfer: bool,
    ) -> Self {
        if administrative_accounts.is_empty() && allow_unrestricted_transfer {
            TransferConfig::Unadministered
        } else {
            TransferConfig::Administered {
                administrative_accounts,
                allow_unrestricted_transfer,
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
                allow_unrestricted_transfer,
                ..
            } => *allow_unrestricted_transfer,
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

    pub fn should_compute_rewards(&self) -> bool {
        self.config.should_compute_rewards
    }

    pub fn into_transfers(self) -> Vec<TransferAddr> {
        self.transfers
    }
}
