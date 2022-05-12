use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};

use casper_engine_test_support::LmdbWasmTestBuilder;
use casper_types::{
    system::{
        auction::{
            Bid, Bids, SeigniorageRecipient, SeigniorageRecipientsSnapshot,
            SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY,
        },
        mint::TOTAL_SUPPLY_KEY,
    },
    AsymmetricType, CLValue, EraId, Key, PublicKey, StoredValue, U512,
};

use crate::utils::{hash_from_str, validators_diff, StateTracker, ValidatorsDiff};

/// A struct tracking all the changes to the state made during the course of the upgrade.
pub struct ValidatorsUpdateManager {
    builder: LmdbWasmTestBuilder,
    state_tracker: StateTracker,
}

impl ValidatorsUpdateManager {
    /// Creates a new `ValidatorsUpdateManager`.
    pub fn new(data_dir: &str, state_hash: &str) -> Self {
        // Open the global state that should be in the supplied directory.
        let builder =
            LmdbWasmTestBuilder::open_raw(data_dir, Default::default(), hash_from_str(state_hash));

        // Find the hash of the mint contract.
        let mint_contract_hash = builder.get_system_mint_hash();
        // Read the URef under which total supply is stored.
        let total_supply_key = builder
            .get_contract(mint_contract_hash)
            .expect("mint should exist")
            .named_keys()[TOTAL_SUPPLY_KEY];
        // Read the total supply.
        let total_supply_sv = builder
            .query(None, total_supply_key, &[])
            .expect("should query");
        let total_supply = total_supply_sv
            .as_cl_value()
            .cloned()
            .expect("should be cl value");

        ValidatorsUpdateManager {
            builder,
            state_tracker: StateTracker::new(
                total_supply_key,
                total_supply.into_t().expect("should be U512"),
            ),
        }
    }

    /// Prints all writes to be performed to the global state.
    pub fn print_writes(&self) {
        self.state_tracker.print_all_entries();
    }

    /// Performs the update, replacing the current validators with the supplied set.
    pub fn perform_update(&mut self, validators: Vec<(String, String)>) {
        // Read the old SeigniorageRecipientsSnapshot
        let (validators_key, old_snapshot) = self.read_snapshot();

        // Create a new snapshot based on the old one and the supplied validators.
        let new_snapshot = gen_snapshot(
            validators,
            *old_snapshot.keys().next().unwrap(),
            old_snapshot.len() as u64,
        );

        // Save the write to the snapshot key.
        self.state_tracker.write_entry(
            validators_key,
            StoredValue::from(CLValue::from_t(new_snapshot.clone()).unwrap()),
        );

        let validators_diff = validators_diff(&old_snapshot, &new_snapshot);

        self.add_and_remove_bids(&validators_diff, &new_snapshot);

        self.remove_withdraws(&validators_diff);
    }

    /// Reads the `SeigniorageRecipientsSnapshot` stored in the global state.
    fn read_snapshot(&self) -> (Key, SeigniorageRecipientsSnapshot) {
        // Find the hash of the auction contract.
        let auction_contract_hash = self.builder.get_system_auction_hash();

        // Read the key under which the snapshot is stored.
        let validators_key = self
            .builder
            .get_contract(auction_contract_hash)
            .expect("auction should exist")
            .named_keys()[SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY];

        // Decode the old snapshot.
        let stored_value = self
            .builder
            .query(None, validators_key, &[])
            .expect("should query");
        let cl_value = stored_value
            .as_cl_value()
            .cloned()
            .expect("should be cl value");
        (validators_key, cl_value.into_t().expect("should convert"))
    }

    /// Generates a set of writes necessary to "fix" the bids, ie.:
    /// - set the bids of the new validators to their desired stakes,
    /// - remove the bids of the old validators that are no longer validators,
    /// - remove all the bids that are larger than the smallest bid among the new validators
    /// (necessary, because such bidders would outbid the validators decided by the social
    /// consensus).
    pub fn add_and_remove_bids(
        &mut self,
        validators_diff: &ValidatorsDiff,
        new_snapshot: &SeigniorageRecipientsSnapshot,
    ) {
        let large_bids = self.find_large_bids(new_snapshot);
        let to_unbid = validators_diff.removed.union(&large_bids);

        let bids = self.builder.get_bids();

        for (pub_key, seigniorage_recipient) in new_snapshot.values().next().unwrap() {
            let stake = *seigniorage_recipient.stake();
            self.create_or_update_bid(&bids, pub_key, stake);
        }

        for pub_key in to_unbid {
            let account_hash = pub_key.to_account_hash();
            if let Some(bid) = bids.get(pub_key) {
                // Replace the bid with an empty bid.
                self.state_tracker.write_entry(
                    Key::Bid(account_hash),
                    Bid::empty(pub_key.clone(), *bid.bonding_purse()).into(),
                );
                // Zero the balance of the bonding purse.
                self.state_tracker.write_entry(
                    Key::Balance(bid.bonding_purse().addr()),
                    StoredValue::CLValue(CLValue::from_t(U512::zero()).unwrap()),
                );
                // Decrease the total supply - we just effectively burned the tokens.
                self.state_tracker.decrease_supply(*bid.staked_amount());
            }
        }
    }

    // Creates a bid for a new validator, or updates the existing one if the validator has already
    // bid.
    // Transfers as much as possible from the validator's purse to the bid purse, and if it's not
    // enough, creates additional tokens.
    fn create_or_update_bid(&mut self, bids: &Bids, pub_key: &PublicKey, stake: U512) {
        let maybe_bid = bids.get(pub_key);
        let bid_amount = maybe_bid
            .map(|bid| *bid.staked_amount())
            .unwrap_or(U512::zero());

        let account_hash = pub_key.to_account_hash();
        let maybe_account = self.builder.get_account(account_hash);
        let account_balance = maybe_account
            .as_ref()
            .map(|acc| self.builder.get_purse_balance(acc.main_purse()))
            .unwrap_or(U512::zero());

        match stake.cmp(&bid_amount) {
            Ordering::Greater => {
                let to_top_up = stake - bid_amount;
                let (to_transfer, to_create) = if to_top_up > account_balance {
                    (account_balance, to_top_up - account_balance)
                } else {
                    (to_top_up, U512::zero())
                };

                if to_transfer > U512::zero() {
                    self.state_tracker.write_entry(
                        // if we can transfer anything at all, the account has to exist, and so the
                        // unwrap is safe
                        Key::Balance(maybe_account.unwrap().main_purse().addr()),
                        StoredValue::CLValue(
                            CLValue::from_t(account_balance - to_transfer).unwrap(),
                        ),
                    );
                }

                let bonding_purse = if let Some(bid) = maybe_bid {
                    // update the balance of the existing purse and increase the supply if we
                    // created any tokens
                    let bonding_purse = *bid.bonding_purse();
                    self.state_tracker.write_entry(
                        Key::Balance(bonding_purse.addr()),
                        StoredValue::CLValue(CLValue::from_t(stake).unwrap()),
                    );
                    if to_create > U512::zero() {
                        self.state_tracker.increase_supply(to_create);
                    }
                    bonding_purse
                } else {
                    // `create_purse` takes care of increasing the supply
                    self.state_tracker.create_purse(stake)
                };

                self.state_tracker.write_entry(
                    Key::Bid(account_hash),
                    Bid::unlocked(pub_key.clone(), bonding_purse, stake, Default::default()).into(),
                );
            }
            Ordering::Less => {
                let to_withdraw = bid_amount - stake;
                let account = maybe_account.expect("a bonded validator should have an account");
                let bid = maybe_bid.expect("a bonded validator should have a bid");
                let bonding_purse = *bid.bonding_purse();
                // withdraw the surplus amount from the bid purse to the account main purse
                self.state_tracker.write_entry(
                    Key::Balance(bonding_purse.addr()),
                    StoredValue::CLValue(CLValue::from_t(stake).unwrap()),
                );
                self.state_tracker.write_entry(
                    Key::Balance(account.main_purse().addr()),
                    StoredValue::CLValue(CLValue::from_t(account_balance + to_withdraw).unwrap()),
                );
                // update the bid
                self.state_tracker.write_entry(
                    Key::Bid(account_hash),
                    Bid::unlocked(pub_key.clone(), bonding_purse, stake, Default::default()).into(),
                );
            }
            // nothing to do if the target bid equals the current bid
            Ordering::Equal => (),
        }
    }

    /// Generates the writes to the global state that will remove the pending withdraws of all the
    /// old validators that will cease to be validators.
    fn remove_withdraws(&mut self, validators_diff: &ValidatorsDiff) {
        let withdraws = self.builder.get_unbonds();
        let withdraw_keys: BTreeSet<_> = withdraws.keys().collect();
        for (key, value) in validators_diff
            .removed
            .iter()
            .map(PublicKey::to_account_hash)
            .filter(|acc| withdraw_keys.contains(&acc))
            .map(|acc| (Key::Withdraw(acc), StoredValue::Withdraw(vec![])))
        {
            self.state_tracker.write_entry(key, value);
        }
    }

    /// Returns the set of public keys that have bids larger than the smallest bid among the new
    /// validators.
    fn find_large_bids(&mut self, snapshot: &SeigniorageRecipientsSnapshot) -> BTreeSet<PublicKey> {
        let seigniorage_recipients = snapshot.values().next().unwrap();
        let min_bid = seigniorage_recipients
            .values()
            .map(SeigniorageRecipient::stake)
            .min()
            .unwrap();
        self.builder
            .get_bids()
            .into_iter()
            .filter(|(pub_key, bid)| {
                bid.staked_amount() >= min_bid && !seigniorage_recipients.contains_key(pub_key)
            })
            .map(|(pub_key, _bid)| pub_key)
            .collect()
    }
}

/// Generates a new `SeigniorageRecipientsSnapshot` based on:
/// - The list of validators, in format (validator_public_key,stake), both expressed as strings.
/// - The starting era ID (the era ID at which the snapshot should start).
/// - Count - the number of eras to be included in the snapshot.
fn gen_snapshot(
    validators: Vec<(String, String)>,
    starting_era_id: EraId,
    count: u64,
) -> SeigniorageRecipientsSnapshot {
    let mut new_snapshot = BTreeMap::new();
    let mut era_validators = BTreeMap::new();
    for (pub_key_str, bonded_amount_str) in &validators {
        let validator_pub_key = PublicKey::from_hex(pub_key_str.as_bytes()).unwrap();
        let bonded_amount = U512::from_dec_str(bonded_amount_str).unwrap();
        let seigniorage_recipient =
            SeigniorageRecipient::new(bonded_amount, Default::default(), Default::default());
        let _ = era_validators.insert(validator_pub_key, seigniorage_recipient);
    }
    for era_id in starting_era_id.iter(count) {
        let _ = new_snapshot.insert(era_id, era_validators.clone());
    }

    new_snapshot
}
