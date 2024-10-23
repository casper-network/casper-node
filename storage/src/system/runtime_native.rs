use crate::{
    global_state::{error::Error as GlobalStateReader, state::StateReader},
    tracking_copy::{TrackingCopyEntityExt, TrackingCopyError, TrackingCopyExt},
    AddressGenerator, TrackingCopy,
};
use casper_types::{
    account::AccountHash, addressable_entity::NamedKeys, Chainspec, ContextAccessRights,
    EntityAddr, FeeHandling, Key, Phase, ProtocolVersion, PublicKey, RefundHandling,
    RuntimeFootprint, StoredValue, TransactionHash, Transfer, URef, U512,
};
use num_rational::Ratio;
use std::{cell::RefCell, collections::BTreeSet, rc::Rc};
use tracing::error;

/// Configuration settings.
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
    balance_hold_interval: u64,
    include_credits: bool,
    credit_cap: Ratio<U512>,
    enable_addressable_entity: bool,
}

impl Config {
    /// Ctor.
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
        balance_hold_interval: u64,
        include_credits: bool,
        credit_cap: Ratio<U512>,
        enable_addressable_entity: bool,
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
            balance_hold_interval,
            include_credits,
            credit_cap,
            enable_addressable_entity,
        }
    }

    /// Ctor from chainspec.
    pub fn from_chainspec(chainspec: &Chainspec) -> Self {
        let transfer_config = TransferConfig::from_chainspec(chainspec);
        let fee_handling = chainspec.core_config.fee_handling;
        let refund_handling = chainspec.core_config.refund_handling;
        let vesting_schedule_period_millis = chainspec.core_config.vesting_schedule_period.millis();
        let allow_auction_bids = chainspec.core_config.allow_auction_bids;
        let compute_rewards = chainspec.core_config.compute_rewards;
        let max_delegators_per_validator = chainspec.core_config.max_delegators_per_validator;
        let minimum_delegation_amount = chainspec.core_config.minimum_delegation_amount;
        let balance_hold_interval = chainspec.core_config.gas_hold_interval.millis();
        let include_credits = chainspec.core_config.fee_handling == FeeHandling::NoFee;
        let credit_cap = Ratio::new_raw(
            U512::from(*chainspec.core_config.validator_credit_cap.numer()),
            U512::from(*chainspec.core_config.validator_credit_cap.denom()),
        );
        let enable_addressable_entity = chainspec.core_config.enable_addressable_entity;
        Config::new(
            transfer_config,
            fee_handling,
            refund_handling,
            vesting_schedule_period_millis,
            allow_auction_bids,
            compute_rewards,
            max_delegators_per_validator,
            minimum_delegation_amount,
            balance_hold_interval,
            include_credits,
            credit_cap,
            enable_addressable_entity,
        )
    }

    /// Returns transfer config.
    pub fn transfer_config(&self) -> &TransferConfig {
        &self.transfer_config
    }

    /// Returns fee handling setting.
    pub fn fee_handling(&self) -> &FeeHandling {
        &self.fee_handling
    }

    /// Returns refund handling setting.
    pub fn refund_handling(&self) -> &RefundHandling {
        &self.refund_handling
    }

    /// Returns vesting schedule period millis setting.
    pub fn vesting_schedule_period_millis(&self) -> u64 {
        self.vesting_schedule_period_millis
    }

    /// Returns if auction bids are allowed.
    pub fn allow_auction_bids(&self) -> bool {
        self.allow_auction_bids
    }

    /// Returns if rewards should be computed.
    pub fn compute_rewards(&self) -> bool {
        self.compute_rewards
    }

    /// Returns max delegators per validator setting.
    pub fn max_delegators_per_validator(&self) -> u32 {
        self.max_delegators_per_validator
    }

    /// Returns minimum delegation amount setting.
    pub fn minimum_delegation_amount(&self) -> u64 {
        self.minimum_delegation_amount
    }

    /// Returns balance hold interval setting.
    pub fn balance_hold_interval(&self) -> u64 {
        self.balance_hold_interval
    }

    /// Returns include credit setting.
    pub fn include_credits(&self) -> bool {
        self.include_credits
    }

    /// Returns validator credit cap setting.
    pub fn credit_cap(&self) -> Ratio<U512> {
        self.credit_cap
    }

    /// Enable the addressable entity and migrate accounts/contracts to entities.
    pub fn enable_addressable_entity(&self) -> bool {
        self.enable_addressable_entity
    }

    /// Changes the transfer config.
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
            balance_hold_interval: self.balance_hold_interval,
            include_credits: self.include_credits,
            credit_cap: self.credit_cap,
            enable_addressable_entity: self.enable_addressable_entity,
        }
    }
}

/// Configuration for transfer.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum TransferConfig {
    /// Transfers are affected by the existence of administrative_accounts. This is a
    /// behavior specific to private or managed chains, not a public chain.
    Administered {
        /// Retrusn the set of account hashes for all administrators.
        administrative_accounts: BTreeSet<AccountHash>,
        /// If true, transfers are unrestricted.
        /// If false, the source and / or target of a transfer must be an administrative account.
        allow_unrestricted_transfers: bool,
    },
    /// Transfers are not affected by the existence of administrative_accounts (the standard
    /// behavior).
    #[default]
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

/// Id for runtime processing.
pub enum Id {
    /// Hash of current transaction.
    Transaction(TransactionHash),
    /// An arbitrary set of bytes to be used as a seed value.
    Seed(Vec<u8>),
}

impl Id {
    /// Ctor for id enum.
    pub fn seed(&self) -> Vec<u8> {
        match self {
            Id::Transaction(hash) => hash.digest().into_vec(),
            Id::Seed(bytes) => bytes.clone(),
        }
    }
}

/// State held by an instance of runtime native.
pub struct RuntimeNative<S> {
    config: Config,
    id: Id,

    address_generator: AddressGenerator,
    protocol_version: ProtocolVersion,

    tracking_copy: Rc<RefCell<TrackingCopy<S>>>,
    address: AccountHash,
    context_key: Key,
    runtime_footprint: RuntimeFootprint,
    access_rights: ContextAccessRights,
    remaining_spending_limit: U512,
    transfers: Vec<Transfer>,
    phase: Phase,
}

impl<S> RuntimeNative<S>
where
    S: StateReader<Key, StoredValue, Error = GlobalStateReader>,
{
    /// Ctor.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: Config,
        protocol_version: ProtocolVersion,
        id: Id,
        tracking_copy: Rc<RefCell<TrackingCopy<S>>>,
        address: AccountHash,
        context_key: Key,
        runtime_footprint: RuntimeFootprint,
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
            context_key,
            runtime_footprint,
            access_rights,
            remaining_spending_limit,
            transfers,
            phase,
        }
    }

    /// Creates a runtime with elevated permissions for systemic behaviors.
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
        let (entity_addr, runtime_footprint, access_rights) = tracking_copy
            .borrow_mut()
            .system_entity_runtime_footprint(protocol_version)?;
        let address = PublicKey::System.to_account_hash();
        let context_key = if config.enable_addressable_entity {
            Key::AddressableEntity(entity_addr)
        } else {
            Key::Hash(entity_addr.value())
        };
        let remaining_spending_limit = U512::MAX; // system has no spending limit
        Ok(RuntimeNative {
            config,
            id,
            address_generator,
            protocol_version,

            tracking_copy,
            address,
            context_key,
            runtime_footprint,
            access_rights,
            remaining_spending_limit,
            transfers,
            phase,
        })
    }

    /// Creates a runtime context for a system contract.
    pub fn new_system_contract_runtime(
        config: Config,
        protocol_version: ProtocolVersion,
        id: Id,
        tracking_copy: Rc<RefCell<TrackingCopy<S>>>,
        phase: Phase,
        name: &str,
    ) -> Result<Self, TrackingCopyError> {
        let seed = id.seed();
        let address_generator = AddressGenerator::new(&seed, phase);
        let transfers = vec![];

        let system_entity_registry = tracking_copy.borrow().get_system_entity_registry()?;
        let hash = match system_entity_registry.get(name).copied() {
            Some(hash) => hash,
            None => {
                error!("unexpected failure; system contract {} not found", name);
                return Err(TrackingCopyError::MissingSystemContractHash(
                    name.to_string(),
                ));
            }
        };
        let context_key = if config.enable_addressable_entity {
            Key::AddressableEntity(EntityAddr::System(hash))
        } else {
            Key::Hash(hash)
        };
        let runtime_footprint = tracking_copy
            .borrow_mut()
            .runtime_footprint_by_hash_addr(hash)?;
        let access_rights =
            runtime_footprint.extract_access_rights(hash, runtime_footprint.named_keys());
        let address = PublicKey::System.to_account_hash();
        let remaining_spending_limit = U512::MAX; // system has no spending limit
        Ok(RuntimeNative {
            config,
            id,
            address_generator,
            protocol_version,

            tracking_copy,
            address,
            context_key,
            runtime_footprint,
            access_rights,
            remaining_spending_limit,
            transfers,
            phase,
        })
    }

    /// Returns mutable reference to address generator.
    pub fn address_generator(&mut self) -> &mut AddressGenerator {
        &mut self.address_generator
    }

    /// Returns reference to config.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Returns reference to transfer config.
    pub fn transfer_config(&self) -> &TransferConfig {
        &self.config.transfer_config
    }

    /// Returns protocol version.
    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }

    /// Returns handle to tracking copy.
    pub fn tracking_copy(&self) -> Rc<RefCell<TrackingCopy<S>>> {
        Rc::clone(&self.tracking_copy)
    }

    /// Returns account hash being used by this instance.
    pub fn address(&self) -> AccountHash {
        self.address
    }

    /// Changes the account hash being used by this instance.
    pub fn with_address(&mut self, account_hash: AccountHash) {
        self.address = account_hash;
    }

    /// Returns the context key being used by this instance.
    pub fn context_key(&self) -> &Key {
        &self.context_key
    }

    /// Returns the addressable entity being used by this instance.
    pub fn runtime_footprint(&self) -> &RuntimeFootprint {
        &self.runtime_footprint
    }

    /// Changes the addressable entity being used by this instance.
    pub fn with_addressable_entity(&mut self, runtime_footprint: RuntimeFootprint) {
        self.runtime_footprint = runtime_footprint;
    }

    /// Returns a reference to the named keys being used by this instance.
    pub fn named_keys(&self) -> &NamedKeys {
        self.runtime_footprint().named_keys()
    }

    /// Returns a mutable reference to the named keys being used by this instance.
    pub fn named_keys_mut(&mut self) -> &mut NamedKeys {
        self.runtime_footprint.named_keys_mut()
    }

    /// Returns a reference to the access rights being used by this instance.
    pub fn access_rights(&self) -> &ContextAccessRights {
        &self.access_rights
    }

    /// Returns a mutable reference to the access rights being used by this instance.
    pub fn access_rights_mut(&mut self) -> &mut ContextAccessRights {
        &mut self.access_rights
    }

    /// Extends the access rights being used by this instance.
    pub fn extend_access_rights(&mut self, urefs: &[URef]) {
        self.access_rights.extend(urefs)
    }

    /// Returns the remaining spending limit.
    pub fn remaining_spending_limit(&self) -> U512 {
        self.remaining_spending_limit
    }

    /// Set remaining spending limit.
    pub fn set_remaining_spending_limit(&mut self, remaining: U512) {
        self.remaining_spending_limit = remaining;
    }

    /// Get references to transfers.
    pub fn transfers(&self) -> &Vec<Transfer> {
        &self.transfers
    }

    /// Push transfer instance.
    pub fn push_transfer(&mut self, transfer: Transfer) {
        self.transfers.push(transfer);
    }

    /// Get id.
    pub fn id(&self) -> &Id {
        &self.id
    }

    /// Get phase.
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Vesting schedule period in milliseconds.
    pub fn vesting_schedule_period_millis(&self) -> u64 {
        self.config.vesting_schedule_period_millis
    }

    /// Are auction bids allowed?
    pub fn allow_auction_bids(&self) -> bool {
        self.config.allow_auction_bids
    }

    /// Are rewards computed?
    pub fn compute_rewards(&self) -> bool {
        self.config.compute_rewards
    }

    /// Extracts transfer items.
    pub fn into_transfers(self) -> Vec<Transfer> {
        self.transfers
    }
}
