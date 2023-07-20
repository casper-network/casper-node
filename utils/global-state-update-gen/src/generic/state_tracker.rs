use std::{
    cmp::Ordering,
    collections::{btree_map::Entry, BTreeMap, BTreeSet},
    convert::TryFrom,
};

use rand::Rng;

use casper_types::{
    account::AccountHash,
    addressable_entity::{ActionThresholds, AssociatedKeys, NamedKeys, Weight},
    package::{
        ContractPackageKind, ContractPackageStatus, ContractVersions, DisabledVersions, Groups,
    },
    system::auction::{Bid, Bids, SeigniorageRecipientsSnapshot, UnbondingPurse},
    AccessRights, AddressableEntity, CLValue, ContractHash, ContractPackageHash, ContractWasmHash,
    EntryPoints, Key, Package, ProtocolVersion, PublicKey, StoredValue, URef, U512,
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
    bids_cache: Option<Bids>,
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
        let total_supply = total_supply_sv
            .as_cl_value()
            .cloned()
            .expect("should be cl value");

        let protocol_version = reader.get_protocol_version();

        Self {
            reader,
            entries_to_write: Default::default(),
            total_supply_key,
            total_supply: total_supply.into_t().expect("should be U512"),
            accounts_cache: BTreeMap::new(),
            unbonds_cache: BTreeMap::new(),
            purses_cache: BTreeMap::new(),
            bids_cache: None,
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

        let contract_hash = ContractHash::new(rng.gen());
        let contract_package_hash = ContractPackageHash::new(rng.gen());
        let contract_wasm_hash = ContractWasmHash::new([0u8; 32]);

        let associated_keys = AssociatedKeys::new(account_hash, Weight::new(1));

        let addressable_entity = AddressableEntity::new(
            contract_package_hash,
            contract_wasm_hash,
            NamedKeys::default(),
            EntryPoints::new(),
            self.protocol_version,
            main_purse,
            associated_keys,
            ActionThresholds::default(),
        );

        let mut contract_package = Package::new(
            URef::new(rng.gen(), AccessRights::READ_ADD_WRITE),
            ContractVersions::default(),
            DisabledVersions::default(),
            Groups::default(),
            ContractPackageStatus::Locked,
            ContractPackageKind::Account(account_hash),
        );

        contract_package
            .insert_contract_version(self.protocol_version.value().major, contract_hash);
        self.write_entry(
            contract_package_hash.into(),
            StoredValue::ContractPackage(contract_package.clone()),
        );

        self.write_entry(
            contract_hash.into(),
            StoredValue::AddressableEntity(addressable_entity.clone()),
        );

        let addressable_entity_by_account_hash = {
            let contract_key: Key = contract_hash.into();
            CLValue::from_t(contract_key).expect("must convert to cl_value")
        };

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
        let cl_value = stored_value
            .as_cl_value()
            .cloned()
            .expect("should be cl value");
        let snapshot: SeigniorageRecipientsSnapshot = cl_value.into_t().expect("should convert");
        self.seigniorage_recipients = Some((validators_key, snapshot.clone()));
        (validators_key, snapshot)
    }

    /// Reads the bids from the global state.
    pub fn get_bids(&mut self) -> Bids {
        if let Some(ref bids) = self.bids_cache {
            bids.clone()
        } else {
            let bids = self.reader.get_bids();
            self.bids_cache = Some(bids.clone());
            bids
        }
    }

    /// Sets the bid for the given account.
    pub fn set_bid(&mut self, public_key: PublicKey, bid: Bid, slash: bool) {
        let maybe_current_bid = self.get_bids().get(&public_key).cloned();

        let new_amount = *bid.staked_amount();
        let old_amount = maybe_current_bid
            .as_ref()
            .map(|bid| self.get_purse_balance(*bid.bonding_purse()))
            .unwrap_or_else(U512::zero);

        // we called `get_bids` above, so `bids_cache` will be `Some`
        self.bids_cache
            .as_mut()
            .unwrap()
            .insert(public_key.clone(), bid.clone());

        let account_hash = public_key.to_account_hash();

        // Replace the bid.
        self.write_entry(Key::Bid(account_hash), bid.clone().into());

        // Update the bonding purses - this will also take care of the total supply changes.
        if let Some(old_bid) = maybe_current_bid {
            // only zero the bonding purse and write the balance to the new purse if the new one is
            // different (just to save unnecessary writes)
            if old_bid.bonding_purse() != bid.bonding_purse() {
                self.set_purse_balance(*old_bid.bonding_purse(), U512::zero());
                self.set_purse_balance(*bid.bonding_purse(), old_amount);
            }
            //
            for (delegator_pub_key, delegator) in old_bid.delegators() {
                // skip zeroing purses of delegators whose purses don't change - that will be
                // handled later
                if bid
                    .delegators()
                    .get(delegator_pub_key)
                    .map(|new_delegator| new_delegator.bonding_purse() == delegator.bonding_purse())
                    .unwrap_or(false)
                {
                    continue;
                }
                // zero the old purse if we are going to transfer the funds to a new purse, or if
                // we're supposed to slash the delegator
                if slash || bid.delegators().contains_key(delegator_pub_key) {
                    // if we're transferring funds to a new purse, set the new purse balance to the
                    // old amount
                    let old_amount = self.get_purse_balance(*delegator.bonding_purse());
                    if let Some(new_delegator) = bid.delegators().get(delegator_pub_key) {
                        self.set_purse_balance(*new_delegator.bonding_purse(), old_amount);
                    }
                    self.set_purse_balance(*delegator.bonding_purse(), U512::zero());
                } else {
                    let amount = self.get_purse_balance(*delegator.bonding_purse());
                    self.create_unbonding_purse(
                        *delegator.bonding_purse(),
                        &public_key,
                        delegator_pub_key,
                        amount,
                    );
                }
            }
        }

        if (slash && new_amount != old_amount) || new_amount > old_amount {
            self.set_purse_balance(*bid.bonding_purse(), new_amount);
        } else if new_amount < old_amount {
            self.create_unbonding_purse(
                *bid.bonding_purse(),
                &public_key,
                &public_key,
                old_amount - new_amount,
            );
        }

        for (delegator_public_key, delegator) in bid.delegators() {
            let old_amount = self.get_purse_balance(*delegator.bonding_purse());
            let new_amount = *delegator.staked_amount();
            if (slash && new_amount != old_amount) || new_amount > old_amount {
                self.set_purse_balance(*delegator.bonding_purse(), *delegator.staked_amount());
            } else if new_amount < old_amount {
                self.create_unbonding_purse(
                    *delegator.bonding_purse(),
                    &public_key,
                    delegator_public_key,
                    old_amount - new_amount,
                );
            }
        }
    }

    /// Generates the writes to the global state that will remove the pending withdraws and unbonds
    /// of all the old validators that will cease to be validators, and slashes their unbonding
    /// purses.
    pub fn remove_withdraws_and_unbonds(&mut self, removed: &BTreeSet<PublicKey>) {
        let withdraws = self.reader.get_withdraws();
        let unbonds = self.reader.get_unbonds();
        for removed_validator in removed {
            let acc = removed_validator.to_account_hash();
            if let Some(withdraw_set) = withdraws.get(&acc) {
                for withdraw in withdraw_set {
                    self.set_purse_balance(*withdraw.bonding_purse(), U512::zero());
                }
                self.write_entry(Key::Withdraw(acc), StoredValue::Withdraw(vec![]));
            }

            if let Some(unbond_set) = unbonds.get(&acc) {
                for unbond in unbond_set {
                    self.set_purse_balance(*unbond.bonding_purse(), U512::zero());
                }
                self.write_entry(Key::Unbond(acc), StoredValue::Unbonding(vec![]));
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
                let existing_purses = self
                    .reader
                    .query(Key::Unbond(account_hash))
                    .and_then(|stored_val| stored_val.as_unbonding().cloned())
                    .unwrap_or_default();
                entry.insert(existing_purses)
            }
        };
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
