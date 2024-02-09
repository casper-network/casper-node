use std::{
    cmp::Ordering,
    collections::{btree_map::Entry, BTreeMap, BTreeSet},
    convert::TryFrom,
};

use rand::Rng;

use casper_types::{
    account::AccountHash,
    addressable_entity::{ActionThresholds, AssociatedKeys, MessageTopics, Weight},
    package::{EntityVersions, Groups, PackageStatus},
    system::auction::{BidAddr, BidKind, BidsExt, SeigniorageRecipientsSnapshot, UnbondingPurse},
    AccessRights, AddressableEntity, AddressableEntityHash, ByteCodeHash, CLValue, EntityKind,
    EntryPoints, Key, Package, PackageHash, ProtocolVersion, PublicKey, StoredValue, URef, U512,
};

use super::{config::Transfer, state_reader::StateReader};

/// A struct tracking changes to be made to the global state.
pub struct StateTracker<T> {
    reader: T,
    entries_to_write: BTreeMap<Key, StoredValue>,
    total_supply: U512,
    total_supply_key: Key,
    accounts_cache: BTreeMap<AccountHash, AddressableEntity>,
    unbonds_cache: BTreeMap<AccountHash, Vec<UnbondingPurse>>,
    purses_cache: BTreeMap<URef, U512>,
    staking: Option<Vec<BidKind>>,
    seigniorage_recipients: Option<(Key, SeigniorageRecipientsSnapshot)>,
    protocol_version: ProtocolVersion,
}

impl<T: StateReader> StateTracker<T> {
    /// Creates a new `StateTracker`.
    pub fn new(mut reader: T) -> Self {
        // Read the URef under which total supply is stored.
        let total_supply_key = reader.get_total_supply_key();

        // Read the total supply.
        let total_supply_sv = reader.query(total_supply_key).expect("should query");
        let total_supply = total_supply_sv.into_cl_value().expect("should be cl value");

        let protocol_version = reader.get_protocol_version();

        Self {
            reader,
            entries_to_write: Default::default(),
            total_supply_key,
            total_supply: total_supply.into_t().expect("should be U512"),
            accounts_cache: BTreeMap::new(),
            unbonds_cache: BTreeMap::new(),
            purses_cache: BTreeMap::new(),
            staking: None,
            seigniorage_recipients: None,
            protocol_version,
        }
    }

    /// Returns all the entries to be written to the global state
    pub fn get_entries(&self) -> BTreeMap<Key, StoredValue> {
        self.entries_to_write.clone()
    }

    /// Stores a write of an entry in the global state.
    pub fn write_entry(&mut self, key: Key, value: StoredValue) {
        let _ = self.entries_to_write.insert(key, value);
    }

    pub fn write_bid(&mut self, bid_kind: BidKind) {
        let bid_addr = match bid_kind.clone() {
            BidKind::Unified(bid) => BidAddr::from(bid.validator_public_key().clone()),
            BidKind::Validator(validator_bid) => {
                BidAddr::from(validator_bid.validator_public_key().clone())
            }
            BidKind::Delegator(delegator) => BidAddr::new_from_public_keys(
                delegator.validator_public_key(),
                Some(delegator.delegator_public_key()),
            ),
        };

        let _ = self
            .entries_to_write
            .insert(bid_addr.into(), bid_kind.into());
    }

    /// Increases the total supply of the tokens in the network.
    pub fn increase_supply(&mut self, to_add: U512) {
        self.total_supply += to_add;
        self.write_entry(
            self.total_supply_key,
            StoredValue::CLValue(CLValue::from_t(self.total_supply).unwrap()),
        );
    }

    /// Decreases the total supply of the tokens in the network.
    pub fn decrease_supply(&mut self, to_sub: U512) {
        self.total_supply -= to_sub;
        self.write_entry(
            self.total_supply_key,
            StoredValue::CLValue(CLValue::from_t(self.total_supply).unwrap()),
        );
    }

    /// Creates a new purse containing the given amount of motes and returns its URef.
    pub fn create_purse(&mut self, amount: U512) -> URef {
        let mut rng = rand::thread_rng();
        let new_purse = URef::new(rng.gen(), AccessRights::READ_ADD_WRITE);

        // Purse URef pointing to `()` so that the owner cannot modify the purse directly.
        self.write_entry(Key::URef(new_purse), StoredValue::CLValue(CLValue::unit()));

        self.set_purse_balance(new_purse, amount);

        new_purse
    }

    /// Gets the balance of the purse, taking into account changes made during the update.
    pub fn get_purse_balance(&mut self, purse: URef) -> U512 {
        match self.purses_cache.get(&purse).cloned() {
            Some(amount) => amount,
            None => {
                let base_key = Key::Balance(purse.addr());
                let amount = self
                    .reader
                    .query(base_key)
                    .map(|v| CLValue::try_from(v).expect("purse balance should be a CLValue"))
                    .map(|cl_value| cl_value.into_t().expect("purse balance should be a U512"))
                    .unwrap_or_else(U512::zero);
                self.purses_cache.insert(purse, amount);
                amount
            }
        }
    }

    /// Sets the balance of the purse.
    pub fn set_purse_balance(&mut self, purse: URef, balance: U512) {
        let current_balance = self.get_purse_balance(purse);

        match balance.cmp(&current_balance) {
            Ordering::Greater => self.increase_supply(balance - current_balance),
            Ordering::Less => self.decrease_supply(current_balance - balance),
            Ordering::Equal => return,
        }

        self.write_entry(
            Key::Balance(purse.addr()),
            StoredValue::CLValue(CLValue::from_t(balance).unwrap()),
        );
        self.purses_cache.insert(purse, balance);
    }

    /// Creates a new account for the given public key and seeds it with the given amount of
    /// tokens.
    pub fn create_addressable_entity_for_account(
        &mut self,
        account_hash: AccountHash,
        amount: U512,
    ) -> AddressableEntity {
        let main_purse = self.create_purse(amount);

        let mut rng = rand::thread_rng();

        let entity_hash = AddressableEntityHash::new(rng.gen());
        let package_hash = PackageHash::new(rng.gen());
        let contract_wasm_hash = ByteCodeHash::new([0u8; 32]);

        let associated_keys = AssociatedKeys::new(account_hash, Weight::new(1));

        let addressable_entity = AddressableEntity::new(
            package_hash,
            contract_wasm_hash,
            EntryPoints::new(),
            self.protocol_version,
            main_purse,
            associated_keys,
            ActionThresholds::default(),
            MessageTopics::default(),
            EntityKind::Account(account_hash),
        );

        let mut contract_package = Package::new(
            URef::new(rng.gen(), AccessRights::READ_ADD_WRITE),
            EntityVersions::default(),
            BTreeSet::default(),
            Groups::default(),
            PackageStatus::Locked,
        );

        contract_package.insert_entity_version(self.protocol_version.value().major, entity_hash);
        self.write_entry(
            package_hash.into(),
            StoredValue::Package(contract_package.clone()),
        );

        let entity_key = addressable_entity.entity_key(entity_hash);

        self.write_entry(
            entity_key,
            StoredValue::AddressableEntity(addressable_entity.clone()),
        );

        let addressable_entity_by_account_hash =
            { CLValue::from_t(entity_key).expect("must convert to cl_value") };

        self.accounts_cache
            .insert(account_hash, addressable_entity.clone());

        self.write_entry(
            Key::Account(account_hash),
            StoredValue::CLValue(addressable_entity_by_account_hash),
        );

        addressable_entity
    }

    /// Gets the account for the given public key.
    pub fn get_account(&mut self, account_hash: &AccountHash) -> Option<AddressableEntity> {
        match self.accounts_cache.entry(*account_hash) {
            Entry::Vacant(vac) => self
                .reader
                .get_account(*account_hash)
                .map(|account| vac.insert(account).clone()),
            Entry::Occupied(occupied) => Some(occupied.into_mut().clone()),
        }
    }

    pub fn execute_transfer(&mut self, transfer: &Transfer) {
        let from_account = if let Some(account) = self.get_account(&transfer.from) {
            account
        } else {
            eprintln!("\"from\" account doesn't exist; transfer: {:?}", transfer);
            return;
        };

        let to_account = if let Some(account) = self.get_account(&transfer.to) {
            account
        } else {
            self.create_addressable_entity_for_account(transfer.to, U512::zero())
        };

        let from_balance = self.get_purse_balance(from_account.main_purse());

        if from_balance < transfer.amount {
            eprintln!(
                "\"from\" account balance insufficient; balance = {}, transfer = {:?}",
                from_balance, transfer
            );
            return;
        }

        let to_balance = self.get_purse_balance(to_account.main_purse());

        self.set_purse_balance(from_account.main_purse(), from_balance - transfer.amount);
        self.set_purse_balance(to_account.main_purse(), to_balance + transfer.amount);
    }

    /// Reads the `SeigniorageRecipientsSnapshot` stored in the global state.
    pub fn read_snapshot(&mut self) -> (Key, SeigniorageRecipientsSnapshot) {
        if let Some(key_and_snapshot) = &self.seigniorage_recipients {
            return key_and_snapshot.clone();
        }

        // Read the key under which the snapshot is stored.
        let validators_key = self.reader.get_seigniorage_recipients_key();

        // Decode the old snapshot.
        let stored_value = self.reader.query(validators_key).expect("should query");
        let cl_value = stored_value.into_cl_value().expect("should be cl value");
        let snapshot: SeigniorageRecipientsSnapshot = cl_value.into_t().expect("should convert");
        self.seigniorage_recipients = Some((validators_key, snapshot.clone()));
        (validators_key, snapshot)
    }

    /// Reads the bids from the global state.
    pub fn get_bids(&mut self) -> Vec<BidKind> {
        if let Some(ref staking) = self.staking {
            staking.clone()
        } else {
            let staking = self.reader.get_bids();
            self.staking = Some(staking.clone());
            staking
        }
    }

    fn existing_bid(&mut self, bid_kind: &BidKind, existing_bids: Vec<BidKind>) -> Option<BidKind> {
        match bid_kind.clone() {
            BidKind::Unified(bid) => existing_bids
                .unified_bid(bid.validator_public_key())
                .map(|existing_bid| BidKind::Unified(Box::new(existing_bid))),
            BidKind::Validator(validator_bid) => existing_bids
                .validator_bid(validator_bid.validator_public_key())
                .map(|existing_validator| BidKind::Validator(Box::new(existing_validator))),
            BidKind::Delegator(delegator_bid) => {
                // this one is a little tricky due to legacy issues.
                match existing_bids.delegator_by_public_keys(
                    delegator_bid.validator_public_key(),
                    delegator_bid.delegator_public_key(),
                ) {
                    Some(existing_delegator) => {
                        Some(BidKind::Delegator(Box::new(existing_delegator)))
                    }
                    None => {
                        if let Some(existing_bid) =
                            existing_bids.unified_bid(delegator_bid.validator_public_key())
                        {
                            existing_bid
                                .delegators()
                                .get(delegator_bid.delegator_public_key())
                                .map(|existing_delegator| {
                                    BidKind::Delegator(Box::new(existing_delegator.clone()))
                                })
                        } else {
                            None
                        }
                    }
                }
            }
        }
    }

    /// Sets the bid for the given account.
    pub fn set_bid(&mut self, bid_kind: BidKind, slash_instead_of_unbonding: bool) {
        let new_stake = bid_kind.staked_amount();
        let bonding_purse = bid_kind.bonding_purse();
        let bids = self.get_bids();

        let maybe_existing_bid = self.existing_bid(&bid_kind, bids);

        let previous_stake = match maybe_existing_bid {
            None => U512::zero(),
            Some(existing_bid) => {
                //let previous_stake = self.get_purse_balance(existing_bid_kind.bonding_purse());
                let previous_stake = existing_bid.staked_amount();
                if existing_bid.bonding_purse() != bonding_purse {
                    self.set_purse_balance(existing_bid.bonding_purse(), U512::zero());
                    self.set_purse_balance(bonding_purse, previous_stake);
                }
                previous_stake
            }
        };

        // we called `get_bids` above, so `staking` will be `Some`
        self.staking.as_mut().unwrap().upsert(bid_kind.clone());

        // Replace the bid (overwrite the previous bid, if any):
        self.write_bid(bid_kind.clone());

        if (slash_instead_of_unbonding && new_stake != previous_stake) || new_stake > previous_stake
        {
            self.set_purse_balance(bid_kind.bonding_purse(), new_stake);
        } else if new_stake < previous_stake {
            let unbonder_key = match bid_kind.delegator_public_key() {
                None => bid_kind.validator_public_key(),
                Some(delegator_public_key) => delegator_public_key,
            };

            let already_unbonded =
                self.already_unbonding_amount(&bid_kind.validator_public_key(), &unbonder_key);

            let amount = previous_stake - new_stake - already_unbonded;
            self.create_unbonding_purse(
                bid_kind.bonding_purse(),
                &bid_kind.validator_public_key(),
                &unbonder_key,
                amount,
            );
        }
    }

    /// Returns the sum of already unbonding purses for the given validator account & unbonder.
    fn already_unbonding_amount(
        &mut self,
        validator_public_key: &PublicKey,
        unbonder_public_key: &PublicKey,
    ) -> U512 {
        let unbonds = self.reader.get_unbonds();
        let account_hash = AccountHash::from(validator_public_key);
        if let Some(purses) = unbonds.get(&account_hash) {
            if let Some(purse) = purses
                .iter()
                .find(|x| x.unbonder_public_key() == unbonder_public_key)
            {
                return *purse.amount();
            }
        }

        let withdrawals = self.reader.get_withdraws();
        if let Some(withdraws) = withdrawals.get(&account_hash) {
            if let Some(withdraw) = withdraws
                .iter()
                .find(|x| x.unbonder_public_key() == unbonder_public_key)
            {
                return *withdraw.amount();
            }
        }

        U512::zero()
    }

    /// Generates the writes to the global state that will remove the pending withdraws and unbonds
    /// of all the old validators that will cease to be validators, and slashes their unbonding
    /// purses.
    pub fn remove_withdraws_and_unbonds(&mut self, removed: &BTreeSet<PublicKey>) {
        let withdraws = self.reader.get_withdraws();
        let unbonds = self.reader.get_unbonds();
        for removed_validator in removed {
            let acc = removed_validator.to_account_hash();
            if let Some(unbond_set) = unbonds.get(&acc) {
                for unbond in unbond_set {
                    self.set_purse_balance(*unbond.bonding_purse(), U512::zero());
                }
                self.write_entry(Key::Unbond(acc), StoredValue::Unbonding(vec![]));
            }
            if let Some(withdraw_set) = withdraws.get(&acc) {
                for withdraw in withdraw_set {
                    self.set_purse_balance(*withdraw.bonding_purse(), U512::zero());
                }
                self.write_entry(Key::Withdraw(acc), StoredValue::Withdraw(vec![]));
            }
        }
    }

    pub fn create_unbonding_purse(
        &mut self,
        bonding_purse: URef,
        validator_key: &PublicKey,
        unbonder_key: &PublicKey,
        amount: U512,
    ) {
        let account_hash = validator_key.to_account_hash();
        let unbonding_era = self.read_snapshot().1.keys().next().copied().unwrap();
        let unbonding_purses = match self.unbonds_cache.entry(account_hash) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                // Fill the cache with the information from the reader when the cache is empty:
                let existing_purses = self
                    .reader
                    .get_unbonds()
                    .get(&account_hash)
                    .cloned()
                    .unwrap_or_default();

                entry.insert(existing_purses)
            }
        };

        if amount == U512::zero() {
            return;
        }

        // Take the first era from the snapshot as the unbonding era.
        let new_purse = UnbondingPurse::new(
            bonding_purse,
            validator_key.clone(),
            unbonder_key.clone(),
            unbonding_era,
            amount,
            None,
        );

        // This doesn't actually transfer or create any funds - the funds will be transferred from
        // the bonding purse to the unbonder's main purse later by the auction contract.
        unbonding_purses.push(new_purse);
        let unbonding_purses = unbonding_purses.clone();
        self.write_entry(
            Key::Unbond(account_hash),
            StoredValue::Unbonding(unbonding_purses),
        );
    }
}
