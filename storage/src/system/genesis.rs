//! Support for a genesis process.
#![allow(unused_imports)]

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

use casper_types::{
    addressable_entity::{
        ActionThresholds, EntityKind, EntityKindTag, MessageTopics, NamedKeyAddr, NamedKeyValue,
        NamedKeys,
    },
    bytesrepr,
    execution::Effects,
    package::{EntityVersions, Groups, PackageStatus},
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
        SystemEntityType, AUCTION, HANDLE_PAYMENT, MINT,
    },
    AccessRights, AddressableEntity, AddressableEntityHash, AdministratorAccount, ByteCode,
    ByteCodeAddr, ByteCodeHash, ByteCodeKind, CLValue, Chainspec, ChainspecRegistry, Digest,
    EntityAddr, EntryPoints, EraId, FeeHandling, GenesisAccount, GenesisConfig,
    GenesisConfigBuilder, Key, Motes, Package, PackageHash, Phase, ProtocolVersion, PublicKey,
    RefundHandling, StoredValue, SystemConfig, SystemEntityRegistry, Tagged, URef, WasmConfig,
    U512,
};

use crate::{
    global_state::state::StateProvider,
    tracking_copy::{TrackingCopy, TrackingCopyError},
    AddressGenerator,
};

const DEFAULT_ADDRESS: [u8; 32] = [0; 32];

const NO_WASM: bool = true;

/// Error returned as a result of a failed genesis process.
#[derive(Clone, Debug)]
pub enum GenesisError {
    /// Error creating a runtime.
    StateUninitialized,
    /// Error obtaining the mint's contract key.
    InvalidMintKey,
    /// Missing mint contract.
    MissingMintContract,
    /// Unexpected stored value variant.
    UnexpectedStoredValue,
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
    /// A bytesrepr Error.
    Bytesrepr(bytesrepr::Error),
    /// Genesis process requires initial accounts.
    MissingGenesisAccounts,
    /// A tracking copy error.
    TrackingCopy(TrackingCopyError),
}

impl fmt::Display for GenesisError {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "GenesisError: {:?}", self)
    }
}

pub struct GenesisInstaller<S>
where
    S: StateProvider + ?Sized,
{
    protocol_version: ProtocolVersion,
    config: GenesisConfig,
    address_generator: Rc<RefCell<AddressGenerator>>,
    tracking_copy: Rc<RefCell<TrackingCopy<<S as StateProvider>::Reader>>>,
}

impl<S> GenesisInstaller<S>
where
    S: StateProvider + ?Sized,
{
    pub fn new(
        genesis_config_hash: Digest,
        protocol_version: ProtocolVersion,
        config: GenesisConfig,
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
            config,
            address_generator,
            tracking_copy,
        }
    }

    pub fn finalize(self) -> Effects {
        self.tracking_copy.borrow().effects()
    }

    fn setup_system_account(&mut self) -> Result<(), Box<GenesisError>> {
        let system_account_addr = PublicKey::System.to_account_hash();

        self.store_addressable_entity(
            EntityKind::Account(system_account_addr),
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
                    self.config.round_seigniorage_rate().into();
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
            EntityKind::System(SystemEntityType::Mint),
        )?;

        {
            // Insert a partial registry into global state.
            // This allows for default values to be accessible when the remaining system contracts
            // call the `call_host_mint` function during their creation.
            let mut partial_registry = BTreeMap::<String, AddressableEntityHash>::new();
            partial_registry.insert(MINT.to_string(), contract_hash);
            partial_registry.insert(HANDLE_PAYMENT.to_string(), DEFAULT_ADDRESS.into());
            let cl_registry = CLValue::from_t(partial_registry)
                .map_err(|error| GenesisError::CLValue(error.to_string()))?;
            self.tracking_copy
                .borrow_mut()
                .write(Key::SystemEntityRegistry, StoredValue::CLValue(cl_registry));
        }

        Ok(total_supply_uref.into())
    }

    fn create_handle_payment(&self) -> Result<AddressableEntityHash, Box<GenesisError>> {
        let handle_payment_payment_purse = self.create_purse(U512::zero())?;
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
            EntityKind::System(SystemEntityType::HandlePayment),
        )?;

        self.store_system_contract_registry(HANDLE_PAYMENT, contract_hash)?;

        Ok(contract_hash)
    }

    fn create_auction(
        &self,
        total_supply_key: Key,
    ) -> Result<AddressableEntityHash, Box<GenesisError>> {
        let locked_funds_period_millis = self.config.locked_funds_period_millis();
        let auction_delay: u64 = self.config.auction_delay();
        let genesis_timestamp_millis: u64 = self.config.genesis_timestamp_millis();

        let mut named_keys = NamedKeys::new();

        let genesis_validators: Vec<_> = self.config.get_bonded_validators().collect();
        if (self.config.validator_slots() as usize) < genesis_validators.len() {
            return Err(GenesisError::InvalidValidatorSlots {
                validators: genesis_validators.len(),
                validator_slots: self.config.validator_slots(),
            }
            .into());
        }

        let genesis_delegators: Vec<_> = self.config.get_bonded_delegators().collect();

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

        let validator_slots = self.config.validator_slots();
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

        let unbonding_delay = self.config.unbonding_delay();
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
            EntityKind::System(SystemEntityType::Auction),
        )?;

        self.store_system_contract_registry(AUCTION, contract_hash)?;

        Ok(contract_hash)
    }

    pub fn create_accounts(&self, total_supply_key: Key) -> Result<(), Box<GenesisError>> {
        let accounts = {
            let mut ret: Vec<GenesisAccount> = self.config.accounts_iter().cloned().collect();
            let system_account = GenesisAccount::system();
            ret.push(system_account);
            ret
        };

        let mut administrative_accounts = self.config.administrative_accounts().peekable();

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

            let main_purse = self.create_purse(account_starting_balance)?;

            self.store_addressable_entity(
                EntityKind::Account(account.account_hash()),
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
        contract_package_kind: EntityKind,
    ) -> Result<AddressableEntityHash, Box<GenesisError>> {
        self.store_addressable_entity(
            contract_package_kind,
            NO_WASM,
            Some(named_keys),
            Some(entry_points),
            self.create_purse(U512::zero())?,
        )
    }

    fn store_addressable_entity(
        &self,
        entity_kind: EntityKind,
        no_wasm: bool,
        maybe_named_keys: Option<NamedKeys>,
        maybe_entry_points: Option<EntryPoints>,
        main_purse: URef,
    ) -> Result<AddressableEntityHash, Box<GenesisError>> {
        let protocol_version = self.protocol_version;
        let byte_code_hash = if no_wasm {
            ByteCodeHash::new(DEFAULT_ADDRESS)
        } else {
            ByteCodeHash::new(self.address_generator.borrow_mut().new_hash_address())
        };
        let entity_hash = if entity_kind.is_system_account() {
            let entity_hash_addr = PublicKey::System.to_account_hash().value();
            AddressableEntityHash::new(entity_hash_addr)
        } else {
            AddressableEntityHash::new(self.address_generator.borrow_mut().new_hash_address())
        };

        let package_hash = PackageHash::new(self.address_generator.borrow_mut().new_hash_address());

        let byte_code = ByteCode::new(ByteCodeKind::Empty, vec![]);
        let associated_keys = entity_kind.associated_keys();
        let maybe_account_hash = entity_kind.maybe_account_hash();
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

        self.store_system_contract_named_keys(entity_hash, named_keys)?;

        let entity = AddressableEntity::new(
            package_hash,
            byte_code_hash,
            entry_points,
            protocol_version,
            main_purse,
            associated_keys,
            ActionThresholds::default(),
            MessageTopics::default(),
            entity_kind,
        );

        let access_key = self
            .address_generator
            .borrow_mut()
            .new_uref(AccessRights::READ_ADD_WRITE);

        // Genesis contracts can be versioned contracts.
        let contract_package = {
            let mut package = Package::new(
                access_key,
                EntityVersions::new(),
                BTreeSet::default(),
                Groups::default(),
                PackageStatus::default(),
            );
            package.insert_entity_version(protocol_version.value().major, entity_hash);
            package
        };

        let byte_code_key = Key::ByteCode(ByteCodeAddr::Empty);

        self.tracking_copy
            .borrow_mut()
            .write(byte_code_key, StoredValue::ByteCode(byte_code));

        let entity_addr = match entity_kind.tag() {
            EntityKindTag::System => EntityAddr::new_system_entity_addr(entity_hash.value()),
            EntityKindTag::Account => EntityAddr::new_account_entity_addr(entity_hash.value()),
            EntityKindTag::SmartContract => {
                EntityAddr::new_contract_entity_addr(entity_hash.value())
            }
        };

        let entity_key: Key = entity_addr.into();

        self.tracking_copy
            .borrow_mut()
            .write(entity_key, StoredValue::AddressableEntity(entity));

        self.tracking_copy
            .borrow_mut()
            .write(package_hash.into(), StoredValue::Package(contract_package));

        if let Some(account_hash) = maybe_account_hash {
            let entity_by_account = CLValue::from_t(entity_key)
                .map_err(|error| GenesisError::CLValue(error.to_string()))?;

            self.tracking_copy.borrow_mut().write(
                Key::Account(account_hash),
                StoredValue::CLValue(entity_by_account),
            );
        }

        Ok(entity_hash)
    }

    fn store_system_contract_named_keys(
        &self,
        contract_hash: AddressableEntityHash,
        named_keys: NamedKeys,
    ) -> Result<(), Box<GenesisError>> {
        let entity_addr = EntityAddr::new_system_entity_addr(contract_hash.value());

        for (string, key) in named_keys.iter() {
            let named_key_entry = NamedKeyAddr::new_from_string(entity_addr, string.clone())
                .map_err(GenesisError::Bytesrepr)?;

            let named_key_value = NamedKeyValue::from_concrete_values(*key, string.clone())
                .map_err(|error| GenesisError::CLValue(error.to_string()))?;

            let entry_key = Key::NamedKey(named_key_entry);

            self.tracking_copy
                .borrow_mut()
                .write(entry_key, StoredValue::NamedKey(named_key_value));
        }

        Ok(())
    }

    fn store_system_contract_registry(
        &self,
        contract_name: &str,
        contract_hash: AddressableEntityHash,
    ) -> Result<(), Box<GenesisError>> {
        let partial_cl_registry = self
            .tracking_copy
            .borrow_mut()
            .read(&Key::SystemEntityRegistry)
            .map_err(|_| GenesisError::FailedToCreateSystemRegistry)?
            .ok_or_else(|| {
                GenesisError::CLValue("failed to convert registry as stored value".to_string())
            })?
            .into_cl_value()
            .ok_or_else(|| GenesisError::CLValue("failed to convert to CLValue".to_string()))?;
        let mut partial_registry = CLValue::into_t::<SystemEntityRegistry>(partial_cl_registry)
            .map_err(|error| GenesisError::CLValue(error.to_string()))?;
        partial_registry.insert(contract_name.to_string(), contract_hash);
        let cl_registry = CLValue::from_t(partial_registry)
            .map_err(|error| GenesisError::CLValue(error.to_string()))?;
        self.tracking_copy
            .borrow_mut()
            .write(Key::SystemEntityRegistry, StoredValue::CLValue(cl_registry));
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
    pub fn install(
        &mut self,
        chainspec_registry: ChainspecRegistry,
    ) -> Result<(), Box<GenesisError>> {
        // Setup system account

        self.setup_system_account()?;

        // Create mint
        let total_supply_key = self.create_mint()?;

        // Create all genesis accounts
        self.create_accounts(total_supply_key)?;

        // Create the auction and setup the stake of all genesis validators.
        self.create_auction(total_supply_key)?;

        // Create handle payment
        self.create_handle_payment()?;

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
