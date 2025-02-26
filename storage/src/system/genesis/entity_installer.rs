use itertools::Itertools;
use num_rational::Ratio;
use num_traits::Zero;
use rand::Rng;
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

use crate::{
    global_state::state::StateProvider,
    system::genesis::{GenesisError, DEFAULT_ADDRESS, NO_WASM},
    AddressGenerator, TrackingCopy,
};
use casper_types::{
    addressable_entity::{
        ActionThresholds, EntityKindTag, MessageTopics, NamedKeyAddr, NamedKeyValue,
    },
    contracts::NamedKeys,
    execution::Effects,
    system::{
        auction,
        auction::{
            BidAddr, BidKind, DelegatorBid, DelegatorKind, SeigniorageRecipient,
            SeigniorageRecipientV2, SeigniorageRecipients, SeigniorageRecipientsSnapshot,
            SeigniorageRecipientsSnapshotV2, SeigniorageRecipientsV2, Staking, ValidatorBid,
            AUCTION_DELAY_KEY, DEFAULT_SEIGNIORAGE_RECIPIENTS_SNAPSHOT_VERSION,
            DELEGATION_RATE_DENOMINATOR, ERA_END_TIMESTAMP_MILLIS_KEY, ERA_ID_KEY,
            INITIAL_ERA_END_TIMESTAMP_MILLIS, INITIAL_ERA_ID, LOCKED_FUNDS_PERIOD_KEY,
            SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY, SEIGNIORAGE_RECIPIENTS_SNAPSHOT_VERSION_KEY,
            UNBONDING_DELAY_KEY, VALIDATOR_SLOTS_KEY,
        },
        handle_payment,
        handle_payment::ACCUMULATION_PURSE_KEY,
        mint,
        mint::{
            ARG_ROUND_SEIGNIORAGE_RATE, MINT_GAS_HOLD_HANDLING_KEY, MINT_GAS_HOLD_INTERVAL_KEY,
            ROUND_SEIGNIORAGE_RATE_KEY, TOTAL_SUPPLY_KEY,
        },
        SystemEntityType, AUCTION, HANDLE_PAYMENT, MINT,
    },
    AccessRights, AddressableEntity, AddressableEntityHash, AdministratorAccount, BlockGlobalAddr,
    ByteCode, ByteCodeAddr, ByteCodeHash, ByteCodeKind, CLValue, ChainspecRegistry, Digest,
    EntityAddr, EntityKind, EntityVersions, EntryPointAddr, EntryPointValue, EntryPoints, EraId,
    GenesisAccount, GenesisConfig, Groups, HashAddr, Key, Motes, Package, PackageHash,
    PackageStatus, Phase, ProtocolVersion, PublicKey, StoredValue, SystemHashRegistry, Tagged,
    URef, U512,
};

pub struct EntityGenesisInstaller<S>
where
    S: StateProvider,
{
    protocol_version: ProtocolVersion,
    config: GenesisConfig,
    address_generator: Rc<RefCell<AddressGenerator>>,
    tracking_copy: Rc<RefCell<TrackingCopy<<S as StateProvider>::Reader>>>,
}

impl<S> EntityGenesisInstaller<S>
where
    S: StateProvider,
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

        EntityGenesisInstaller {
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

        let gas_hold_handling_uref =
            {
                let gas_hold_handling = self.config.gas_hold_balance_handling().tag();
                let gas_hold_handling_uref = self
                    .address_generator
                    .borrow_mut()
                    .new_uref(AccessRights::READ_ADD_WRITE);

                self.tracking_copy.borrow_mut().write(
                    gas_hold_handling_uref.into(),
                    StoredValue::CLValue(CLValue::from_t(gas_hold_handling).map_err(|_| {
                        GenesisError::CLValue(MINT_GAS_HOLD_HANDLING_KEY.to_string())
                    })?),
                );
                gas_hold_handling_uref
            };

        let gas_hold_interval_uref =
            {
                let gas_hold_interval = self.config.gas_hold_interval_millis();
                let gas_hold_interval_uref = self
                    .address_generator
                    .borrow_mut()
                    .new_uref(AccessRights::READ_ADD_WRITE);

                self.tracking_copy.borrow_mut().write(
                    gas_hold_interval_uref.into(),
                    StoredValue::CLValue(CLValue::from_t(gas_hold_interval).map_err(|_| {
                        GenesisError::CLValue(MINT_GAS_HOLD_INTERVAL_KEY.to_string())
                    })?),
                );
                gas_hold_interval_uref
            };

        let named_keys = {
            let mut named_keys = NamedKeys::new();
            named_keys.insert(
                ROUND_SEIGNIORAGE_RATE_KEY.to_string(),
                round_seigniorage_rate_uref.into(),
            );
            named_keys.insert(TOTAL_SUPPLY_KEY.to_string(), total_supply_uref.into());
            named_keys.insert(
                MINT_GAS_HOLD_HANDLING_KEY.to_string(),
                gas_hold_handling_uref.into(),
            );
            named_keys.insert(
                MINT_GAS_HOLD_INTERVAL_KEY.to_string(),
                gas_hold_interval_uref.into(),
            );
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

    fn create_handle_payment(&self) -> Result<HashAddr, Box<GenesisError>> {
        let handle_payment_payment_purse = self.create_purse(U512::zero())?;
        let named_keys = {
            let mut named_keys = NamedKeys::new();
            let named_key = Key::URef(handle_payment_payment_purse);
            named_keys.insert(handle_payment::PAYMENT_PURSE_KEY.to_string(), named_key);

            // This purse is used only in FeeHandling::Accumulate setting.
            let accumulation_purse_uref = self.create_purse(U512::zero())?;
            named_keys.insert(
                ACCUMULATION_PURSE_KEY.to_string(),
                accumulation_purse_uref.into(),
            );
            named_keys
        };

        let entry_points = handle_payment::handle_payment_entry_points();

        let contract_hash = self.store_system_contract(
            named_keys,
            entry_points,
            EntityKind::System(SystemEntityType::HandlePayment),
        )?;

        self.store_system_entity_registry(HANDLE_PAYMENT, contract_hash.value())?;

        Ok(contract_hash.value())
    }

    fn create_auction(&self, total_supply_key: Key) -> Result<HashAddr, Box<GenesisError>> {
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
            if *delegated_amount == &Motes::zero() {
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
                let mut delegators = BTreeMap::new();

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
                        0,
                        u64::MAX,
                        0,
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

                            let delegator_kind: DelegatorKind =
                                (*delegator_public_key).clone().into();
                            let delegator = DelegatorBid::locked(
                                delegator_kind.clone(),
                                delegator_delegated_amount.value(),
                                purse_uref,
                                (*validator_public_key).clone(),
                                release_timestamp_millis,
                            );

                            if delegators.insert(delegator_kind, delegator).is_some() {
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

        // initialize snapshot version flag
        let initial_seigniorage_recipients_version_uref = self
            .address_generator
            .borrow_mut()
            .new_uref(AccessRights::READ_ADD_WRITE);
        self.tracking_copy.borrow_mut().write(
            initial_seigniorage_recipients_version_uref.into(),
            StoredValue::CLValue(
                CLValue::from_t(DEFAULT_SEIGNIORAGE_RECIPIENTS_SNAPSHOT_VERSION).map_err(|_| {
                    GenesisError::CLValue(SEIGNIORAGE_RECIPIENTS_SNAPSHOT_VERSION_KEY.to_string())
                })?,
            ),
        );

        named_keys.insert(
            SEIGNIORAGE_RECIPIENTS_SNAPSHOT_VERSION_KEY.into(),
            initial_seigniorage_recipients_version_uref.into(),
        );

        // store all delegator and validator bids
        for (validator_public_key, (validator_bid, delegators)) in staked {
            for (delegator_kind, delegator_bid) in delegators {
                let delegator_bid_key = Key::BidAddr(BidAddr::new_delegator_kind(
                    &validator_public_key,
                    &delegator_kind,
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

        self.store_system_entity_registry(AUCTION, contract_hash.value())?;

        Ok(contract_hash.value())
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
    ) -> BTreeMap<EraId, SeigniorageRecipientsV2> {
        let initial_snapshot_range = INITIAL_ERA_ID.iter_inclusive(auction_delay);

        let mut seigniorage_recipients = SeigniorageRecipientsV2::new();
        for (validator_public_key, (validator_bid, delegators)) in staked {
            let mut delegator_stake = BTreeMap::new();
            for (k, v) in delegators {
                delegator_stake.insert(k.clone(), v.staked_amount());
            }
            let recipient = SeigniorageRecipientV2::new(
                validator_bid.staked_amount(),
                *validator_bid.delegation_rate(),
                delegator_stake,
                BTreeMap::new(),
            );
            seigniorage_recipients.insert(validator_public_key.clone(), recipient);
        }

        let mut initial_seigniorage_recipients = SeigniorageRecipientsSnapshotV2::new();
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

        let entity_hash = match entity_kind {
            EntityKind::System(_) | EntityKind::SmartContract(_) => {
                AddressableEntityHash::new(self.address_generator.borrow_mut().new_hash_address())
            }
            EntityKind::Account(account_hash) => {
                if entity_kind.is_system_account() {
                    let entity_hash_addr = PublicKey::System.to_account_hash().value();
                    AddressableEntityHash::new(entity_hash_addr)
                } else {
                    AddressableEntityHash::new(account_hash.value())
                }
            }
        };

        let package_hash = PackageHash::new(self.address_generator.borrow_mut().new_hash_address());

        let byte_code = ByteCode::new(ByteCodeKind::Empty, vec![]);
        let associated_keys = entity_kind.associated_keys();
        let maybe_account_hash = entity_kind.maybe_account_hash();
        let named_keys = maybe_named_keys.unwrap_or_default();

        self.store_system_contract_named_keys(entity_hash, named_keys)?;
        if let Some(entry_point) = maybe_entry_points {
            self.store_system_entry_points(entity_hash, entry_point)?;
        }

        let entity = AddressableEntity::new(
            package_hash,
            byte_code_hash,
            protocol_version,
            main_purse,
            associated_keys,
            ActionThresholds::default(),
            entity_kind,
        );

        // Genesis contracts can be versioned contracts.
        let contract_package = {
            let mut package = Package::new(
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
            EntityKindTag::System => EntityAddr::new_system(entity_hash.value()),
            EntityKindTag::Account => EntityAddr::new_account(entity_hash.value()),
            EntityKindTag::SmartContract => EntityAddr::new_smart_contract(entity_hash.value()),
        };

        let entity_key: Key = entity_addr.into();

        self.tracking_copy
            .borrow_mut()
            .write(entity_key, StoredValue::AddressableEntity(entity));

        self.tracking_copy.borrow_mut().write(
            package_hash.into(),
            StoredValue::SmartContract(contract_package),
        );

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
        let entity_addr = EntityAddr::new_system(contract_hash.value());

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

    fn store_system_entry_points(
        &self,
        contract_hash: AddressableEntityHash,
        entry_points: EntryPoints,
    ) -> Result<(), Box<GenesisError>> {
        let entity_addr = EntityAddr::new_system(contract_hash.value());

        for entry_point in entry_points.take_entry_points() {
            let entry_point_addr =
                EntryPointAddr::new_v1_entry_point_addr(entity_addr, entry_point.name())
                    .map_err(GenesisError::Bytesrepr)?;
            self.tracking_copy.borrow_mut().write(
                Key::EntryPoint(entry_point_addr),
                StoredValue::EntryPoint(EntryPointValue::V1CasperVm(entry_point)),
            )
        }

        Ok(())
    }

    fn store_system_entity_registry(
        &self,
        contract_name: &str,
        contract_hash: HashAddr,
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
        let mut partial_registry = CLValue::into_t::<SystemHashRegistry>(partial_cl_registry)
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

    /// Writes a tracking record to global state for block time / genesis timestamp.
    fn store_block_time(&self) -> Result<(), Box<GenesisError>> {
        let cl_value = CLValue::from_t(self.config.genesis_timestamp_millis())
            .map_err(|error| GenesisError::CLValue(error.to_string()))?;

        self.tracking_copy.borrow_mut().write(
            Key::BlockGlobal(BlockGlobalAddr::BlockTime),
            StoredValue::CLValue(cl_value),
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

        // Write chainspec registry.
        self.store_chainspec_registry(chainspec_registry)?;

        // Write block time to global state
        self.store_block_time()?;

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

        let genesis_account_1 = GenesisAccount::account(public_key.clone(), Motes::new(100), None);

        bytesrepr::test_serialization_roundtrip(&genesis_account_1);

        let genesis_account_2 =
            GenesisAccount::account(public_key, Motes::new(100), Some(rng.gen()));

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
            Motes::new(100),
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
