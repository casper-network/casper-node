//! Support for a genesis process.
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    fmt, iter,
    rc::Rc,
};

use itertools::Itertools;
use num::Zero;
use num_rational::Ratio;
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
use serde::{Deserialize, Serialize};

use casper_storage::global_state::state::StateProvider;
use casper_types::{
    addressable_entity::{ActionThresholds, NamedKeys},
    execution::Effects,
    package::{ContractPackageKind, ContractPackageStatus, ContractVersions, Groups},
    system::{
        auction::{
            self, BidAddr, BidKind, DelegationRate, Delegator, SeigniorageRecipient,
            SeigniorageRecipients, SeigniorageRecipientsSnapshot, Staking, ValidatorBid,
            AUCTION_DELAY_KEY, DELEGATION_RATE_DENOMINATOR, ERA_END_TIMESTAMP_MILLIS_KEY,
            ERA_ID_KEY, INITIAL_ERA_END_TIMESTAMP_MILLIS, INITIAL_ERA_ID, LOCKED_FUNDS_PERIOD_KEY,
            SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY, UNBONDING_DELAY_KEY, VALIDATOR_SLOTS_KEY,
        },
        handle_payment::{self, ACCUMULATION_PURSE_KEY},
        mint::{self, ARG_ROUND_SEIGNIORAGE_RATE, ROUND_SEIGNIORAGE_RATE_KEY, TOTAL_SUPPLY_KEY},
        standard_payment, SystemContractType, AUCTION, HANDLE_PAYMENT, MINT, STANDARD_PAYMENT,
    },
    AccessRights, AddressableEntity, AdministratorAccount, CLValue, Chainspec, ChainspecRegistry,
    ContractHash, ContractPackageHash, ContractWasm, ContractWasmHash, Digest, EntryPoints, EraId,
    FeeHandling, GenesisAccount, Key, Motes, Package, Phase, ProtocolVersion, PublicKey,
    RefundHandling, StoredValue, SystemConfig, URef, WasmConfig, U512,
};

use crate::{
    engine_state::{SystemContractRegistry, DEFAULT_ADDRESS},
    execution::{self, AddressGenerator},
    tracking_copy::TrackingCopy,
};

use super::engine_config::{DEFAULT_FEE_HANDLING, DEFAULT_REFUND_HANDLING};

const NO_WASM: bool = true;

/// Default number of validator slots.
pub const DEFAULT_VALIDATOR_SLOTS: u32 = 5;
/// Default auction delay.
pub const DEFAULT_AUCTION_DELAY: u64 = 1;
/// Default lock-in period is currently zero.
pub const DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS: u64 = 0;
/// Default number of eras that need to pass to be able to withdraw unbonded funds.
pub const DEFAULT_UNBONDING_DELAY: u64 = 7;
/// Default round seigniorage rate represented as a fractional number.
///
/// Annual issuance: 2%
/// Minimum round exponent: 14
/// Ticks per year: 31536000000
///
/// (1+0.02)^((2^14)/31536000000)-1 is expressed as a fraction below.
pub const DEFAULT_ROUND_SEIGNIORAGE_RATE: Ratio<u64> = Ratio::new_raw(6414, 623437335209);
/// Default genesis timestamp in milliseconds.
pub const DEFAULT_GENESIS_TIMESTAMP_MILLIS: u64 = 0;

/// Represents an outcome of a successful genesis run.
#[derive(Debug)]
pub struct GenesisSuccess {
    /// State hash after genesis is committed to the global state.
    pub post_state_hash: Digest,
    /// Effects of a successful genesis.
    pub effects: Effects,
}

impl fmt::Display for GenesisSuccess {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "Success: {} {:?}", self.post_state_hash, self.effects)
    }
}

/// Represents a configuration of a genesis process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenesisConfig {
    name: String,
    timestamp: u64,
    protocol_version: ProtocolVersion,
    ee_config: ExecConfig,
}

impl GenesisConfig {
    /// Creates a new genesis config object.
    pub fn new(
        name: String,
        timestamp: u64,
        protocol_version: ProtocolVersion,
        ee_config: ExecConfig,
    ) -> Self {
        GenesisConfig {
            name,
            timestamp,
            protocol_version,
            ee_config,
        }
    }

    /// Returns name.
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Returns timestamp.
    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    /// Returns protocol version.
    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }

    /// Returns configuration details of the genesis process.
    pub fn ee_config(&self) -> &ExecConfig {
        &self.ee_config
    }

    /// Returns mutable reference to the configuration.
    pub fn ee_config_mut(&mut self) -> &mut ExecConfig {
        &mut self.ee_config
    }

    /// Returns genesis configuration and consumes the object.
    pub fn take_ee_config(self) -> ExecConfig {
        self.ee_config
    }
}

impl Distribution<GenesisConfig> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> GenesisConfig {
        let count = rng.gen_range(1..1000);
        let name = iter::repeat(())
            .map(|_| rng.gen::<char>())
            .take(count)
            .collect();

        let timestamp = rng.gen();

        let protocol_version = ProtocolVersion::from_parts(rng.gen(), rng.gen(), rng.gen());

        let ee_config = rng.gen();

        GenesisConfig {
            name,
            timestamp,
            protocol_version,
            ee_config,
        }
    }
}

/// Represents the details of a genesis process.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecConfig {
    accounts: Vec<GenesisAccount>,
    wasm_config: WasmConfig,
    system_config: SystemConfig,
    validator_slots: u32,
    auction_delay: u64,
    locked_funds_period_millis: u64,
    round_seigniorage_rate: Ratio<u64>,
    unbonding_delay: u64,
    genesis_timestamp_millis: u64,
    refund_handling: RefundHandling,
    fee_handling: FeeHandling,
}

impl ExecConfig {
    /// Creates a new genesis configuration.
    ///
    /// New code should use [`ExecConfigBuilder`] instead as some config options will otherwise be
    /// defaulted.
    #[deprecated(
        since = "3.0.0",
        note = "prefer to use ExecConfigBuilder to construct an ExecConfig"
    )]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        accounts: Vec<GenesisAccount>,
        wasm_config: WasmConfig,
        system_config: SystemConfig,
        validator_slots: u32,
        auction_delay: u64,
        locked_funds_period_millis: u64,
        round_seigniorage_rate: Ratio<u64>,
        unbonding_delay: u64,
        genesis_timestamp_millis: u64,
    ) -> ExecConfig {
        ExecConfig {
            accounts,
            wasm_config,
            system_config,
            validator_slots,
            auction_delay,
            locked_funds_period_millis,
            round_seigniorage_rate,
            unbonding_delay,
            genesis_timestamp_millis,
            refund_handling: DEFAULT_REFUND_HANDLING,
            fee_handling: DEFAULT_FEE_HANDLING,
        }
    }

    /// Returns WASM config.
    pub fn wasm_config(&self) -> &WasmConfig {
        &self.wasm_config
    }

    /// Returns system config.
    pub fn system_config(&self) -> &SystemConfig {
        &self.system_config
    }

    /// Returns all bonded genesis validators.
    pub fn get_bonded_validators(&self) -> impl Iterator<Item = &GenesisAccount> {
        self.accounts_iter()
            .filter(|&genesis_account| genesis_account.is_validator())
    }

    /// Returns all bonded genesis delegators.
    pub fn get_bonded_delegators(
        &self,
    ) -> impl Iterator<Item = (&PublicKey, &PublicKey, &Motes, &Motes)> {
        self.accounts
            .iter()
            .filter_map(|genesis_account| genesis_account.as_delegator())
    }

    /// Returns all genesis accounts.
    pub fn accounts(&self) -> &[GenesisAccount] {
        self.accounts.as_slice()
    }

    /// Returns an iterator over all genesis accounts.
    pub(crate) fn accounts_iter(&self) -> impl Iterator<Item = &GenesisAccount> {
        self.accounts.iter()
    }

    /// Returns an iterator over all administrative accounts.
    pub(crate) fn administrative_accounts(&self) -> impl Iterator<Item = &AdministratorAccount> {
        self.accounts
            .iter()
            .filter_map(GenesisAccount::as_administrator_account)
    }

    /// Adds new genesis account to the config.
    pub fn push_account(&mut self, account: GenesisAccount) {
        self.accounts.push(account)
    }

    /// Returns validator slots.
    pub fn validator_slots(&self) -> u32 {
        self.validator_slots
    }

    /// Returns auction delay.
    pub fn auction_delay(&self) -> u64 {
        self.auction_delay
    }

    /// Returns locked funds period expressed in milliseconds.
    pub fn locked_funds_period_millis(&self) -> u64 {
        self.locked_funds_period_millis
    }

    /// Returns round seigniorage rate.
    pub fn round_seigniorage_rate(&self) -> Ratio<u64> {
        self.round_seigniorage_rate
    }

    /// Returns unbonding delay in eras.
    pub fn unbonding_delay(&self) -> u64 {
        self.unbonding_delay
    }

    /// Returns genesis timestamp expressed in milliseconds.
    pub fn genesis_timestamp_millis(&self) -> u64 {
        self.genesis_timestamp_millis
    }
}

impl Distribution<ExecConfig> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> ExecConfig {
        let count = rng.gen_range(1..10);

        let accounts = iter::repeat(()).map(|_| rng.gen()).take(count).collect();

        let wasm_config = rng.gen();

        let system_config = rng.gen();

        let validator_slots = rng.gen();

        let auction_delay = rng.gen();

        let locked_funds_period_millis = rng.gen();

        let round_seigniorage_rate = Ratio::new(
            rng.gen_range(1..1_000_000_000),
            rng.gen_range(1..1_000_000_000),
        );

        let unbonding_delay = rng.gen();

        let genesis_timestamp_millis = rng.gen();

        let refund_handling = RefundHandling::Refund {
            refund_ratio: Ratio::new_raw(rng.gen_range(0..=100), 100),
        };

        let fee_handling = if rng.gen() {
            FeeHandling::Accumulate
        } else {
            FeeHandling::PayToProposer
        };

        ExecConfig {
            accounts,
            wasm_config,
            system_config,
            validator_slots,
            auction_delay,
            locked_funds_period_millis,
            round_seigniorage_rate,
            unbonding_delay,
            genesis_timestamp_millis,
            refund_handling,
            fee_handling,
        }
    }
}

/// A builder for an [`ExecConfig`].
///
/// Any field that isn't specified will be defaulted.  See [the module docs](index.html) for the set
/// of default values.
#[derive(Default, Debug)]
pub struct ExecConfigBuilder {
    accounts: Option<Vec<GenesisAccount>>,
    wasm_config: Option<WasmConfig>,
    system_config: Option<SystemConfig>,
    validator_slots: Option<u32>,
    auction_delay: Option<u64>,
    locked_funds_period_millis: Option<u64>,
    round_seigniorage_rate: Option<Ratio<u64>>,
    unbonding_delay: Option<u64>,
    genesis_timestamp_millis: Option<u64>,
    refund_handling: Option<RefundHandling>,
    fee_handling: Option<FeeHandling>,
}

impl ExecConfigBuilder {
    /// Creates a new `ExecConfig` builder.
    pub fn new() -> Self {
        ExecConfigBuilder::default()
    }

    /// Sets the genesis accounts.
    pub fn with_accounts(mut self, accounts: Vec<GenesisAccount>) -> Self {
        self.accounts = Some(accounts);
        self
    }

    /// Sets the Wasm config options.
    pub fn with_wasm_config(mut self, wasm_config: WasmConfig) -> Self {
        self.wasm_config = Some(wasm_config);
        self
    }

    /// Sets the system config options.
    pub fn with_system_config(mut self, system_config: SystemConfig) -> Self {
        self.system_config = Some(system_config);
        self
    }

    /// Sets the validator slots config option.
    pub fn with_validator_slots(mut self, validator_slots: u32) -> Self {
        self.validator_slots = Some(validator_slots);
        self
    }

    /// Sets the auction delay config option.
    pub fn with_auction_delay(mut self, auction_delay: u64) -> Self {
        self.auction_delay = Some(auction_delay);
        self
    }

    /// Sets the locked funds period config option.
    pub fn with_locked_funds_period_millis(mut self, locked_funds_period_millis: u64) -> Self {
        self.locked_funds_period_millis = Some(locked_funds_period_millis);
        self
    }

    /// Sets the round seigniorage rate config option.
    pub fn with_round_seigniorage_rate(mut self, round_seigniorage_rate: Ratio<u64>) -> Self {
        self.round_seigniorage_rate = Some(round_seigniorage_rate);
        self
    }

    /// Sets the unbonding delay config option.
    pub fn with_unbonding_delay(mut self, unbonding_delay: u64) -> Self {
        self.unbonding_delay = Some(unbonding_delay);
        self
    }

    /// Sets the genesis timestamp config option.
    pub fn with_genesis_timestamp_millis(mut self, genesis_timestamp_millis: u64) -> Self {
        self.genesis_timestamp_millis = Some(genesis_timestamp_millis);
        self
    }

    /// Sets the refund handling config option.
    pub fn with_refund_handling(mut self, refund_handling: RefundHandling) -> Self {
        self.refund_handling = Some(refund_handling);
        self
    }

    /// Sets the fee handling config option.
    pub fn with_fee_handling(mut self, fee_handling: FeeHandling) -> Self {
        self.fee_handling = Some(fee_handling);
        self
    }

    /// Builds a new [`ExecConfig`] object.
    pub fn build(self) -> ExecConfig {
        ExecConfig {
            accounts: self.accounts.unwrap_or_default(),
            wasm_config: self.wasm_config.unwrap_or_default(),
            system_config: self.system_config.unwrap_or_default(),
            validator_slots: self.validator_slots.unwrap_or(DEFAULT_VALIDATOR_SLOTS),
            auction_delay: self.auction_delay.unwrap_or(DEFAULT_AUCTION_DELAY),
            locked_funds_period_millis: self
                .locked_funds_period_millis
                .unwrap_or(DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS),
            round_seigniorage_rate: self
                .round_seigniorage_rate
                .unwrap_or(DEFAULT_ROUND_SEIGNIORAGE_RATE),
            unbonding_delay: self.unbonding_delay.unwrap_or(DEFAULT_UNBONDING_DELAY),
            genesis_timestamp_millis: self
                .genesis_timestamp_millis
                .unwrap_or(DEFAULT_GENESIS_TIMESTAMP_MILLIS),
            refund_handling: self.refund_handling.unwrap_or(DEFAULT_REFUND_HANDLING),
            fee_handling: self.fee_handling.unwrap_or(DEFAULT_FEE_HANDLING),
        }
    }
}

impl From<&Chainspec> for ExecConfig {
    fn from(chainspec: &Chainspec) -> Self {
        let genesis_timestamp_millis = chainspec
            .protocol_config
            .activation_point
            .genesis_timestamp()
            .map_or(0, |timestamp| timestamp.millis());

        ExecConfigBuilder::default()
            .with_accounts(chainspec.network_config.accounts_config.clone().into())
            .with_wasm_config(chainspec.wasm_config)
            .with_system_config(chainspec.system_costs_config)
            .with_validator_slots(chainspec.core_config.validator_slots)
            .with_auction_delay(chainspec.core_config.auction_delay)
            .with_locked_funds_period_millis(chainspec.core_config.locked_funds_period.millis())
            .with_round_seigniorage_rate(chainspec.core_config.round_seigniorage_rate)
            .with_unbonding_delay(chainspec.core_config.unbonding_delay)
            .with_genesis_timestamp_millis(genesis_timestamp_millis)
            .with_refund_handling(chainspec.core_config.refund_handling)
            .with_fee_handling(chainspec.core_config.fee_handling)
            .build()
    }
}

/// Error returned as a result of a failed genesis process.
#[derive(Clone, Debug)]
pub enum GenesisError {
    /// Error creating a runtime.
    UnableToCreateRuntime,
    /// Error obtaining the mint's contract key.
    InvalidMintKey,
    /// Missing mint contract.
    MissingMintContract,
    /// Unexpected stored value variant.
    UnexpectedStoredValue,
    /// Error executing a system contract.
    ExecutionError(execution::Error),
    /// Error executing the mint system contract.
    MintError(mint::Error),
    /// Error converting a [`CLValue`] to a concrete type.
    CLValue(String),
    /// Specified validator does not exist among the genesis accounts.
    OrphanedDelegator {
        /// Validator's public key.
        validator_public_key: PublicKey,
        /// Delegator's public key.
        delegator_public_key: PublicKey,
    },
    /// Duplicated delegator entry found for a given validator.
    DuplicatedDelegatorEntry {
        /// Validator's public key.
        validator_public_key: PublicKey,
        /// Delegator's public key.
        delegator_public_key: PublicKey,
    },
    /// Delegation rate outside the allowed range.
    InvalidDelegationRate {
        /// Delegator's public key.
        public_key: PublicKey,
        /// Invalid delegation rate specified in the genesis account entry.
        delegation_rate: DelegationRate,
    },
    /// Invalid bond amount in a genesis account.
    InvalidBondAmount {
        /// Validator's public key.
        public_key: PublicKey,
    },
    /// Invalid delegated amount in a genesis account.
    InvalidDelegatedAmount {
        /// Delegator's public key.
        public_key: PublicKey,
    },
    /// Failed to create system registry.
    FailedToCreateSystemRegistry,
    /// Missing system contract hash.
    MissingSystemContractHash(String),
    /// Invalid number of validator slots configured.
    InvalidValidatorSlots {
        /// Number of validators in the genesis config.
        validators: usize,
        /// Number of validator slots specified.
        validator_slots: u32,
    },
    /// The chainspec registry is missing a required entry.
    MissingChainspecRegistryEntry,
    /// Duplicated administrator entry.
    ///
    /// This error can occur only on some private chains.
    DuplicatedAdministratorEntry,
}

pub(crate) struct GenesisInstaller<S>
where
    S: StateProvider,
    S::Error: Into<execution::Error>,
{
    protocol_version: ProtocolVersion,
    exec_config: ExecConfig,
    address_generator: Rc<RefCell<AddressGenerator>>,
    tracking_copy: Rc<RefCell<TrackingCopy<<S as StateProvider>::Reader>>>,
}

impl<S> GenesisInstaller<S>
where
    S: StateProvider,
    S::Error: Into<execution::Error>,
{
    pub(crate) fn new(
        genesis_config_hash: Digest,
        protocol_version: ProtocolVersion,
        exec_config: ExecConfig,
        tracking_copy: Rc<RefCell<TrackingCopy<<S as StateProvider>::Reader>>>,
    ) -> Self {
        let phase = Phase::System;
        let genesis_config_hash_bytes = genesis_config_hash.as_ref();

        let address_generator = {
            let generator = AddressGenerator::new(genesis_config_hash_bytes, phase);
            Rc::new(RefCell::new(generator))
        };

        GenesisInstaller {
            protocol_version,
            exec_config,
            address_generator,
            tracking_copy,
        }
    }

    pub(crate) fn finalize(self) -> Effects {
        self.tracking_copy.borrow().effects()
    }

    fn setup_system_account(&mut self) -> Result<(), Box<GenesisError>> {
        let system_account_addr = PublicKey::System.to_account_hash();

        self.store_contract(
            ContractPackageKind::Account(system_account_addr),
            NO_WASM,
            None,
            None,
            self.create_purse(U512::zero())?,
        )?;

        Ok(())
    }

    fn create_mint(&mut self) -> Result<Key, Box<GenesisError>> {
        let round_seigniorage_rate_uref =
            {
                let round_seigniorage_rate_uref = self
                    .address_generator
                    .borrow_mut()
                    .new_uref(AccessRights::READ_ADD_WRITE);

                let (round_seigniorage_rate_numer, round_seigniorage_rate_denom) =
                    self.exec_config.round_seigniorage_rate().into();
                let round_seigniorage_rate: Ratio<U512> = Ratio::new(
                    round_seigniorage_rate_numer.into(),
                    round_seigniorage_rate_denom.into(),
                );

                self.tracking_copy.borrow_mut().write(
                    round_seigniorage_rate_uref.into(),
                    StoredValue::CLValue(CLValue::from_t(round_seigniorage_rate).map_err(
                        |_| GenesisError::CLValue(ARG_ROUND_SEIGNIORAGE_RATE.to_string()),
                    )?),
                );
                round_seigniorage_rate_uref
            };

        let total_supply_uref = {
            let total_supply_uref = self
                .address_generator
                .borrow_mut()
                .new_uref(AccessRights::READ_ADD_WRITE);

            self.tracking_copy.borrow_mut().write(
                total_supply_uref.into(),
                StoredValue::CLValue(
                    CLValue::from_t(U512::zero())
                        .map_err(|_| GenesisError::CLValue(TOTAL_SUPPLY_KEY.to_string()))?,
                ),
            );
            total_supply_uref
        };

        let named_keys = {
            let mut named_keys = NamedKeys::new();
            named_keys.insert(
                ROUND_SEIGNIORAGE_RATE_KEY.to_string(),
                round_seigniorage_rate_uref.into(),
            );
            named_keys.insert(TOTAL_SUPPLY_KEY.to_string(), total_supply_uref.into());

            named_keys
        };

        let entry_points = mint::mint_entry_points();

        let contract_hash = self.store_system_contract(
            named_keys,
            entry_points,
            ContractPackageKind::System(SystemContractType::Mint),
        )?;

        {
            // Insert a partial registry into global state.
            // This allows for default values to be accessible when the remaining system contracts
            // call the `call_host_mint` function during their creation.
            let mut partial_registry = BTreeMap::<String, ContractHash>::new();
            partial_registry.insert(MINT.to_string(), contract_hash);
            partial_registry.insert(HANDLE_PAYMENT.to_string(), DEFAULT_ADDRESS.into());
            let cl_registry = CLValue::from_t(partial_registry)
                .map_err(|error| GenesisError::CLValue(error.to_string()))?;
            self.tracking_copy.borrow_mut().write(
                Key::SystemContractRegistry,
                StoredValue::CLValue(cl_registry),
            );
        }

        Ok(total_supply_uref.into())
    }

    fn create_handle_payment(
        &self,
        handle_payment_payment_purse: URef,
    ) -> Result<ContractHash, Box<GenesisError>> {
        let named_keys = {
            let mut named_keys = NamedKeys::new();
            let named_key = Key::URef(handle_payment_payment_purse);
            named_keys.insert(handle_payment::PAYMENT_PURSE_KEY.to_string(), named_key);

            // This purse is used only in FeeHandling::Accumulate setting.
            let rewards_purse_uref = self.create_purse(U512::zero())?;

            named_keys.insert(
                ACCUMULATION_PURSE_KEY.to_string(),
                rewards_purse_uref.into(),
            );

            named_keys
        };

        let entry_points = handle_payment::handle_payment_entry_points();

        let contract_hash = self.store_system_contract(
            named_keys,
            entry_points,
            ContractPackageKind::System(SystemContractType::HandlePayment),
        )?;

        self.store_system_contract_registry(HANDLE_PAYMENT, contract_hash)?;

        Ok(contract_hash)
    }

    fn create_auction(&self, total_supply_key: Key) -> Result<ContractHash, Box<GenesisError>> {
        let locked_funds_period_millis = self.exec_config.locked_funds_period_millis();
        let auction_delay: u64 = self.exec_config.auction_delay();
        let genesis_timestamp_millis: u64 = self.exec_config.genesis_timestamp_millis();

        let mut named_keys = NamedKeys::new();

        let genesis_validators: Vec<_> = self.exec_config.get_bonded_validators().collect();
        if (self.exec_config.validator_slots() as usize) < genesis_validators.len() {
            return Err(GenesisError::InvalidValidatorSlots {
                validators: genesis_validators.len(),
                validator_slots: self.exec_config.validator_slots(),
            }
            .into());
        }

        let genesis_delegators: Vec<_> = self.exec_config.get_bonded_delegators().collect();

        // Make sure all delegators have corresponding genesis validator entries
        for (validator_public_key, delegator_public_key, _, delegated_amount) in
            genesis_delegators.iter()
        {
            if delegated_amount.is_zero() {
                return Err(GenesisError::InvalidDelegatedAmount {
                    public_key: (*delegator_public_key).clone(),
                }
                .into());
            }

            let orphan_condition = genesis_validators.iter().find(|genesis_validator| {
                genesis_validator.public_key() == (*validator_public_key).clone()
            });

            if orphan_condition.is_none() {
                return Err(GenesisError::OrphanedDelegator {
                    validator_public_key: (*validator_public_key).clone(),
                    delegator_public_key: (*delegator_public_key).clone(),
                }
                .into());
            }
        }

        let mut total_staked_amount = U512::zero();

        let staked = {
            let mut staked: Staking = BTreeMap::new();

            for genesis_validator in genesis_validators {
                let public_key = genesis_validator.public_key();
                let mut delegators: BTreeMap<PublicKey, Delegator> = BTreeMap::new();

                let staked_amount = genesis_validator.staked_amount().value();
                if staked_amount.is_zero() {
                    return Err(GenesisError::InvalidBondAmount { public_key }.into());
                }

                let delegation_rate = genesis_validator.delegation_rate();
                if delegation_rate > DELEGATION_RATE_DENOMINATOR {
                    return Err(GenesisError::InvalidDelegationRate {
                        public_key,
                        delegation_rate,
                    }
                    .into());
                }
                debug_assert_ne!(public_key, PublicKey::System);

                total_staked_amount += staked_amount;

                let purse_uref = self.create_purse(staked_amount)?;
                let release_timestamp_millis =
                    genesis_timestamp_millis + locked_funds_period_millis;
                let validator_bid = {
                    let bid = ValidatorBid::locked(
                        public_key.clone(),
                        purse_uref,
                        staked_amount,
                        delegation_rate,
                        release_timestamp_millis,
                    );

                    // Set up delegator entries attached to genesis validators
                    for (
                        validator_public_key,
                        delegator_public_key,
                        _delegator_balance,
                        delegator_delegated_amount,
                    ) in genesis_delegators.iter()
                    {
                        if (*validator_public_key).clone() == public_key.clone() {
                            let purse_uref =
                                self.create_purse(delegator_delegated_amount.value())?;

                            let delegator = Delegator::locked(
                                (*delegator_public_key).clone(),
                                delegator_delegated_amount.value(),
                                purse_uref,
                                (*validator_public_key).clone(),
                                release_timestamp_millis,
                            );

                            if delegators
                                .insert((*delegator_public_key).clone(), delegator)
                                .is_some()
                            {
                                return Err(GenesisError::DuplicatedDelegatorEntry {
                                    validator_public_key: (*validator_public_key).clone(),
                                    delegator_public_key: (*delegator_public_key).clone(),
                                }
                                .into());
                            }
                        }
                    }

                    bid
                };

                staked.insert(public_key, (validator_bid, delegators));
            }
            staked
        };

        let _ = self.tracking_copy.borrow_mut().add(
            total_supply_key,
            StoredValue::CLValue(
                CLValue::from_t(total_staked_amount)
                    .map_err(|_| GenesisError::CLValue(TOTAL_SUPPLY_KEY.to_string()))?,
            ),
        );

        let initial_seigniorage_recipients =
            self.initial_seigniorage_recipients(&staked, auction_delay);

        let era_id_uref = self
            .address_generator
            .borrow_mut()
            .new_uref(AccessRights::READ_ADD_WRITE);
        self.tracking_copy.borrow_mut().write(
            era_id_uref.into(),
            StoredValue::CLValue(
                CLValue::from_t(INITIAL_ERA_ID)
                    .map_err(|_| GenesisError::CLValue(ERA_ID_KEY.to_string()))?,
            ),
        );
        named_keys.insert(ERA_ID_KEY.into(), era_id_uref.into());

        let era_end_timestamp_millis_uref = self
            .address_generator
            .borrow_mut()
            .new_uref(AccessRights::READ_ADD_WRITE);
        self.tracking_copy.borrow_mut().write(
            era_end_timestamp_millis_uref.into(),
            StoredValue::CLValue(
                CLValue::from_t(INITIAL_ERA_END_TIMESTAMP_MILLIS)
                    .map_err(|_| GenesisError::CLValue(ERA_END_TIMESTAMP_MILLIS_KEY.to_string()))?,
            ),
        );
        named_keys.insert(
            ERA_END_TIMESTAMP_MILLIS_KEY.into(),
            era_end_timestamp_millis_uref.into(),
        );

        let initial_seigniorage_recipients_uref = self
            .address_generator
            .borrow_mut()
            .new_uref(AccessRights::READ_ADD_WRITE);
        self.tracking_copy.borrow_mut().write(
            initial_seigniorage_recipients_uref.into(),
            StoredValue::CLValue(CLValue::from_t(initial_seigniorage_recipients).map_err(
                |_| GenesisError::CLValue(SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY.to_string()),
            )?),
        );
        named_keys.insert(
            SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY.into(),
            initial_seigniorage_recipients_uref.into(),
        );

        // store all delegator and validator bids
        for (validator_public_key, (validator_bid, delegators)) in staked {
            for (delegator_public_key, delegator_bid) in delegators {
                let delegator_bid_key = Key::BidAddr(BidAddr::new_from_public_keys(
                    &validator_public_key.clone(),
                    Some(&delegator_public_key.clone()),
                ));
                self.tracking_copy.borrow_mut().write(
                    delegator_bid_key,
                    StoredValue::BidKind(BidKind::Delegator(Box::new(delegator_bid))),
                );
            }
            let validator_bid_key = Key::BidAddr(BidAddr::from(validator_public_key.clone()));
            self.tracking_copy.borrow_mut().write(
                validator_bid_key,
                StoredValue::BidKind(BidKind::Validator(Box::new(validator_bid))),
            );
        }

        let validator_slots = self.exec_config.validator_slots();
        let validator_slots_uref = self
            .address_generator
            .borrow_mut()
            .new_uref(AccessRights::READ_ADD_WRITE);
        self.tracking_copy.borrow_mut().write(
            validator_slots_uref.into(),
            StoredValue::CLValue(
                CLValue::from_t(validator_slots)
                    .map_err(|_| GenesisError::CLValue(VALIDATOR_SLOTS_KEY.to_string()))?,
            ),
        );
        named_keys.insert(VALIDATOR_SLOTS_KEY.into(), validator_slots_uref.into());

        let auction_delay_uref = self
            .address_generator
            .borrow_mut()
            .new_uref(AccessRights::READ_ADD_WRITE);
        self.tracking_copy.borrow_mut().write(
            auction_delay_uref.into(),
            StoredValue::CLValue(
                CLValue::from_t(auction_delay)
                    .map_err(|_| GenesisError::CLValue(AUCTION_DELAY_KEY.to_string()))?,
            ),
        );
        named_keys.insert(AUCTION_DELAY_KEY.into(), auction_delay_uref.into());

        let locked_funds_period_uref = self
            .address_generator
            .borrow_mut()
            .new_uref(AccessRights::READ_ADD_WRITE);
        self.tracking_copy.borrow_mut().write(
            locked_funds_period_uref.into(),
            StoredValue::CLValue(
                CLValue::from_t(locked_funds_period_millis)
                    .map_err(|_| GenesisError::CLValue(LOCKED_FUNDS_PERIOD_KEY.to_string()))?,
            ),
        );
        named_keys.insert(
            LOCKED_FUNDS_PERIOD_KEY.into(),
            locked_funds_period_uref.into(),
        );

        let unbonding_delay = self.exec_config.unbonding_delay();
        let unbonding_delay_uref = self
            .address_generator
            .borrow_mut()
            .new_uref(AccessRights::READ_ADD_WRITE);
        self.tracking_copy.borrow_mut().write(
            unbonding_delay_uref.into(),
            StoredValue::CLValue(
                CLValue::from_t(unbonding_delay)
                    .map_err(|_| GenesisError::CLValue(UNBONDING_DELAY_KEY.to_string()))?,
            ),
        );
        named_keys.insert(UNBONDING_DELAY_KEY.into(), unbonding_delay_uref.into());

        let entry_points = auction::auction_entry_points();

        let contract_hash = self.store_system_contract(
            named_keys,
            entry_points,
            ContractPackageKind::System(SystemContractType::Auction),
        )?;

        self.store_system_contract_registry(AUCTION, contract_hash)?;

        Ok(contract_hash)
    }

    fn create_standard_payment(&self) -> Result<ContractHash, Box<GenesisError>> {
        let named_keys = NamedKeys::new();

        let entry_points = standard_payment::standard_payment_entry_points();

        let contract_hash = self.store_system_contract(
            named_keys,
            entry_points,
            ContractPackageKind::System(SystemContractType::HandlePayment),
        )?;

        self.store_system_contract_registry(STANDARD_PAYMENT, contract_hash)?;

        Ok(contract_hash)
    }

    pub(crate) fn create_accounts(
        &self,
        total_supply_key: Key,
        payment_purse_uref: URef,
    ) -> Result<(), Box<GenesisError>> {
        let accounts = {
            let mut ret: Vec<GenesisAccount> = self.exec_config.accounts_iter().cloned().collect();
            let system_account = GenesisAccount::system();
            ret.push(system_account);
            ret
        };

        let mut administrative_accounts = self.exec_config.administrative_accounts().peekable();

        if administrative_accounts.peek().is_some()
            && administrative_accounts
                .duplicates_by(|admin| admin.public_key())
                .next()
                .is_some()
        {
            // Ensure no duplicate administrator accounts are specified as this might raise errors
            // during genesis process when administrator accounts are added to associated keys.
            return Err(GenesisError::DuplicatedAdministratorEntry.into());
        }

        let mut total_supply = U512::zero();

        for account in accounts {
            let account_starting_balance = account.balance().value();

            let main_purse = match account {
                GenesisAccount::System
                    if self.exec_config.administrative_accounts().next().is_some() =>
                {
                    payment_purse_uref
                }
                _ => self.create_purse(account_starting_balance)?,
            };

            self.store_contract(
                ContractPackageKind::Account(account.account_hash()),
                NO_WASM,
                None,
                None,
                main_purse,
            )?;

            total_supply += account_starting_balance;
        }

        self.tracking_copy.borrow_mut().write(
            total_supply_key,
            StoredValue::CLValue(
                CLValue::from_t(total_supply)
                    .map_err(|_| GenesisError::CLValue(TOTAL_SUPPLY_KEY.to_string()))?,
            ),
        );

        Ok(())
    }

    fn initial_seigniorage_recipients(
        &self,
        staked: &Staking,
        auction_delay: u64,
    ) -> BTreeMap<EraId, SeigniorageRecipients> {
        let initial_snapshot_range = INITIAL_ERA_ID.iter_inclusive(auction_delay);

        let mut seigniorage_recipients = SeigniorageRecipients::new();
        for (validator_public_key, (validator_bid, delegators)) in staked {
            let mut delegator_stake: BTreeMap<PublicKey, U512> = BTreeMap::new();
            for (k, v) in delegators {
                delegator_stake.insert(k.clone(), v.staked_amount());
            }
            let recipient = SeigniorageRecipient::new(
                validator_bid.staked_amount(),
                *validator_bid.delegation_rate(),
                delegator_stake,
            );
            seigniorage_recipients.insert(validator_public_key.clone(), recipient);
        }

        let mut initial_seigniorage_recipients = SeigniorageRecipientsSnapshot::new();
        for era_id in initial_snapshot_range {
            initial_seigniorage_recipients.insert(era_id, seigniorage_recipients.clone());
        }
        initial_seigniorage_recipients
    }

    fn create_purse(&self, amount: U512) -> Result<URef, Box<GenesisError>> {
        let purse_addr = self.address_generator.borrow_mut().create_address();

        let balance_cl_value =
            CLValue::from_t(amount).map_err(|error| GenesisError::CLValue(error.to_string()))?;
        self.tracking_copy.borrow_mut().write(
            Key::Balance(purse_addr),
            StoredValue::CLValue(balance_cl_value),
        );

        let purse_cl_value = CLValue::unit();
        let purse_uref = URef::new(purse_addr, AccessRights::READ_ADD_WRITE);
        self.tracking_copy
            .borrow_mut()
            .write(Key::URef(purse_uref), StoredValue::CLValue(purse_cl_value));

        Ok(purse_uref)
    }

    fn store_system_contract(
        &self,
        named_keys: NamedKeys,
        entry_points: EntryPoints,
        contract_package_kind: ContractPackageKind,
    ) -> Result<ContractHash, Box<GenesisError>> {
        self.store_contract(
            contract_package_kind,
            NO_WASM,
            Some(named_keys),
            Some(entry_points),
            self.create_purse(U512::zero())?,
        )
    }

    fn store_contract(
        &self,
        contract_package_kind: ContractPackageKind,
        no_wasm: bool,
        maybe_named_keys: Option<NamedKeys>,
        maybe_entry_points: Option<EntryPoints>,
        main_purse: URef,
    ) -> Result<ContractHash, Box<GenesisError>> {
        let protocol_version = self.protocol_version;
        let contract_wasm_hash = if no_wasm {
            ContractWasmHash::new(DEFAULT_ADDRESS)
        } else {
            ContractWasmHash::new(self.address_generator.borrow_mut().new_hash_address())
        };
        let contract_hash = if contract_package_kind.is_system_account() {
            let entity_hash_addr = PublicKey::System.to_account_hash().value();
            ContractHash::new(entity_hash_addr)
        } else {
            ContractHash::new(self.address_generator.borrow_mut().new_hash_address())
        };

        let contract_package_hash =
            ContractPackageHash::new(self.address_generator.borrow_mut().new_hash_address());

        let contract_wasm = ContractWasm::new(vec![]);
        let associated_keys = contract_package_kind.associated_keys();
        let maybe_account_hash = contract_package_kind.maybe_account_hash();
        let named_keys = maybe_named_keys.unwrap_or_default();
        let entry_points = match maybe_entry_points {
            Some(entry_points) => entry_points,
            None => {
                if maybe_account_hash.is_some() {
                    EntryPoints::new()
                } else {
                    EntryPoints::new_with_default_entry_point()
                }
            }
        };

        let entity = AddressableEntity::new(
            contract_package_hash,
            contract_wasm_hash,
            named_keys,
            entry_points,
            protocol_version,
            main_purse,
            associated_keys,
            ActionThresholds::default(),
        );

        let access_key = self
            .address_generator
            .borrow_mut()
            .new_uref(AccessRights::READ_ADD_WRITE);

        // Genesis contracts can be versioned contracts.
        let contract_package = {
            let mut contract_package = Package::new(
                access_key,
                ContractVersions::new(),
                BTreeSet::default(),
                Groups::default(),
                ContractPackageStatus::default(),
                contract_package_kind,
            );
            contract_package.insert_contract_version(protocol_version.value().major, contract_hash);
            contract_package
        };

        self.tracking_copy.borrow_mut().write(
            contract_wasm_hash.into(),
            StoredValue::ContractWasm(contract_wasm),
        );
        self.tracking_copy
            .borrow_mut()
            .write(contract_hash.into(), StoredValue::AddressableEntity(entity));
        self.tracking_copy.borrow_mut().write(
            contract_package_hash.into(),
            StoredValue::ContractPackage(contract_package),
        );
        if let Some(account_hash) = maybe_account_hash {
            let contract_key: Key = contract_hash.into();
            let contract_by_account = CLValue::from_t(contract_key)
                .map_err(|error| GenesisError::CLValue(error.to_string()))?;

            self.tracking_copy.borrow_mut().write(
                Key::Account(account_hash),
                StoredValue::CLValue(contract_by_account),
            );
        }

        Ok(contract_hash)
    }

    fn store_system_contract_registry(
        &self,
        contract_name: &str,
        contract_hash: ContractHash,
    ) -> Result<(), Box<GenesisError>> {
        let partial_cl_registry = self
            .tracking_copy
            .borrow_mut()
            .read(&Key::SystemContractRegistry)
            .map_err(|_| GenesisError::FailedToCreateSystemRegistry)?
            .ok_or_else(|| {
                GenesisError::CLValue("failed to convert registry as stored value".to_string())
            })?
            .into_cl_value()
            .ok_or_else(|| GenesisError::CLValue("failed to convert to CLValue".to_string()))?;
        let mut partial_registry = CLValue::into_t::<SystemContractRegistry>(partial_cl_registry)
            .map_err(|error| GenesisError::CLValue(error.to_string()))?;
        partial_registry.insert(contract_name.to_string(), contract_hash);
        let cl_registry = CLValue::from_t(partial_registry)
            .map_err(|error| GenesisError::CLValue(error.to_string()))?;
        self.tracking_copy.borrow_mut().write(
            Key::SystemContractRegistry,
            StoredValue::CLValue(cl_registry),
        );
        Ok(())
    }

    fn store_chainspec_registry(
        &self,
        chainspec_registry: ChainspecRegistry,
    ) -> Result<(), Box<GenesisError>> {
        if chainspec_registry.genesis_accounts_raw_hash().is_none() {
            return Err(GenesisError::MissingChainspecRegistryEntry.into());
        }
        let cl_value_registry = CLValue::from_t(chainspec_registry)
            .map_err(|error| GenesisError::CLValue(error.to_string()))?;

        self.tracking_copy.borrow_mut().write(
            Key::ChainspecRegistry,
            StoredValue::CLValue(cl_value_registry),
        );
        Ok(())
    }

    /// Performs a complete system installation.
    pub(crate) fn install(
        &mut self,
        chainspec_registry: ChainspecRegistry,
    ) -> Result<(), Box<GenesisError>> {
        // Setup system account
        self.setup_system_account()?;

        // Create mint
        let total_supply_key = self.create_mint()?;

        let payment_purse_uref = self.create_purse(U512::zero())?;

        // Create all genesis accounts
        self.create_accounts(total_supply_key, payment_purse_uref)?;

        // Create the auction and setup the stake of all genesis validators.
        self.create_auction(total_supply_key)?;

        // Create handle payment
        self.create_handle_payment(payment_purse_uref)?;

        // Create standard payment
        self.create_standard_payment()?;

        self.store_chainspec_registry(chainspec_registry)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use casper_types::AsymmetricType;
    use rand::RngCore;

    use casper_types::{bytesrepr, SecretKey};

    #[test]
    fn bytesrepr_roundtrip() {
        let mut rng = rand::thread_rng();
        let genesis_account: GenesisAccount = rng.gen();
        bytesrepr::test_serialization_roundtrip(&genesis_account);
    }

    #[test]
    fn system_account_bytesrepr_roundtrip() {
        let genesis_account = GenesisAccount::system();

        bytesrepr::test_serialization_roundtrip(&genesis_account);
    }

    #[test]
    fn genesis_account_bytesrepr_roundtrip() {
        let mut rng = rand::thread_rng();
        let mut bytes = [0u8; 32];
        rng.fill_bytes(&mut bytes[..]);
        let secret_key = SecretKey::ed25519_from_bytes(bytes).unwrap();
        let public_key: PublicKey = PublicKey::from(&secret_key);

        let genesis_account_1 =
            GenesisAccount::account(public_key.clone(), Motes::new(U512::from(100)), None);

        bytesrepr::test_serialization_roundtrip(&genesis_account_1);

        let genesis_account_2 =
            GenesisAccount::account(public_key, Motes::new(U512::from(100)), Some(rng.gen()));

        bytesrepr::test_serialization_roundtrip(&genesis_account_2);
    }

    #[test]
    fn delegator_bytesrepr_roundtrip() {
        let mut rng = rand::thread_rng();
        let mut validator_bytes = [0u8; 32];
        let mut delegator_bytes = [0u8; 32];
        rng.fill_bytes(&mut validator_bytes[..]);
        rng.fill_bytes(&mut delegator_bytes[..]);
        let validator_secret_key = SecretKey::ed25519_from_bytes(validator_bytes).unwrap();
        let delegator_secret_key = SecretKey::ed25519_from_bytes(delegator_bytes).unwrap();

        let validator_public_key = PublicKey::from(&validator_secret_key);
        let delegator_public_key = PublicKey::from(&delegator_secret_key);

        let genesis_account = GenesisAccount::delegator(
            validator_public_key,
            delegator_public_key,
            Motes::new(U512::from(100)),
            Motes::zero(),
        );

        bytesrepr::test_serialization_roundtrip(&genesis_account);
    }

    #[test]
    fn administrator_account_bytesrepr_roundtrip() {
        let administrator_account = AdministratorAccount::new(
            PublicKey::ed25519_from_bytes([123u8; 32]).unwrap(),
            Motes::new(U512::MAX),
        );
        bytesrepr::test_serialization_roundtrip(&administrator_account);
    }
}
