pub(crate) mod config;
mod state_reader;
mod state_tracker;
#[cfg(test)]
mod testing;
mod update;

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};

use casper_engine_test_support::LmdbWasmTestBuilder;
use casper_types::{
    system::auction::{Bid, Delegator, SeigniorageRecipient, SeigniorageRecipientsSnapshot},
    CLValue, EraId, PublicKey, StoredValue, U512,
};

use clap::ArgMatches;

use crate::utils::{hash_from_str, validators_diff, ValidatorInfo, ValidatorsDiff};

use self::{
    config::{AccountConfig, Config, Transfer},
    state_reader::StateReader,
    state_tracker::StateTracker,
    update::Update,
};

pub(crate) fn generate_generic_update(matches: &ArgMatches<'_>) {
    let data_dir = matches.value_of("data_dir").unwrap_or(".");
    let state_hash = hash_from_str(matches.value_of("hash").unwrap());
    let config_path = matches.value_of("config_file").unwrap();

    let config_bytes = fs::read(config_path).expect("couldn't read the config file");
    let config: Config = toml::from_slice(&config_bytes).expect("couldn't parse the config file");

    let builder = LmdbWasmTestBuilder::open_raw(data_dir, Default::default(), state_hash);

    update_from_config(builder, config);
}

fn get_update<T: StateReader>(reader: T, config: Config) -> Update {
    let mut state_tracker = StateTracker::new(reader);

    process_transfers(&mut state_tracker, &config.transfers);

    update_account_balances(&mut state_tracker, &config.accounts);

    let validators = update_auction_state(
        &mut state_tracker,
        &config.accounts,
        config.only_listed_validators,
        config.slash_instead_of_unbonding,
    );

    let entries = state_tracker.get_entries();

    Update::new(entries, validators)
}

pub(crate) fn update_from_config<T: StateReader>(reader: T, config: Config) {
    let update = get_update(reader, config);
    update.print();
}

fn process_transfers<T: StateReader>(state: &mut StateTracker<T>, transfers: &[Transfer]) {
    for transfer in transfers {
        state.execute_transfer(transfer);
    }
}

fn update_account_balances<T: StateReader>(
    state: &mut StateTracker<T>,
    accounts: &[AccountConfig],
) {
    for account in accounts {
        let target_balance = if let Some(balance) = account.balance {
            balance
        } else {
            continue;
        };
        let account_hash = account.public_key.to_account_hash();
        if let Some(account) = state.get_account(&account_hash) {
            state.set_purse_balance(account.main_purse(), target_balance);
        } else {
            state.create_account(account_hash, target_balance);
        }
    }
}

/// Returns the complete set of validators immediately after the upgrade,
/// if the validator set changed.
fn update_auction_state<T: StateReader>(
    state: &mut StateTracker<T>,
    accounts: &[AccountConfig],
    only_listed_validators: bool,
    slash: bool,
) -> Option<Vec<ValidatorInfo>> {
    // Read the old SeigniorageRecipientsSnapshot
    let (validators_key, old_snapshot) = state.read_snapshot();

    // Create a new snapshot based on the old one and the supplied validators.
    let new_snapshot = if only_listed_validators {
        gen_snapshot_only_listed(
            *old_snapshot.keys().next().unwrap(),
            old_snapshot.len() as u64,
            accounts,
        )
    } else {
        gen_snapshot_from_old(old_snapshot.clone(), accounts)
    };

    if new_snapshot == old_snapshot {
        return None;
    }

    // Save the write to the snapshot key.
    state.write_entry(
        validators_key,
        StoredValue::from(CLValue::from_t(new_snapshot.clone()).unwrap()),
    );

    let validators_diff = validators_diff(&old_snapshot, &new_snapshot);

    add_and_remove_bids(
        state,
        &validators_diff,
        &new_snapshot,
        only_listed_validators,
        slash,
    );

    state.remove_withdraws(&validators_diff.removed);

    // We need to output the validators for the next era, which are contained in the first entry
    // in the snapshot.
    Some(
        new_snapshot
            .values()
            .next()
            .expect("snapshot should have at least one entry")
            .iter()
            .map(|(public_key, seigniorage_recipient)| ValidatorInfo {
                public_key: public_key.clone(),
                weight: seigniorage_recipient
                    .total_stake()
                    .expect("total validator stake too large"),
            })
            .collect(),
    )
}

/// Generates a new `SeigniorageRecipientsSnapshot` based on:
/// - The starting era ID (the era ID at which the snapshot should start).
/// - Count - the number of eras to be included in the snapshot.
/// - The list of configured accounts.
fn gen_snapshot_only_listed(
    starting_era_id: EraId,
    count: u64,
    accounts: &[AccountConfig],
) -> SeigniorageRecipientsSnapshot {
    let mut new_snapshot = BTreeMap::new();
    let mut era_validators = BTreeMap::new();
    for account in accounts {
        // don't add validators with zero stake to the snapshot
        let validator_cfg = match &account.validator {
            Some(validator) if validator.bonded_amount != U512::zero() => validator,
            _ => continue,
        };
        let seigniorage_recipient = SeigniorageRecipient::new(
            validator_cfg.bonded_amount,
            validator_cfg.delegation_rate.unwrap_or_default(),
            validator_cfg.delegators_map().unwrap_or_default(),
        );
        let _ = era_validators.insert(account.public_key.clone(), seigniorage_recipient);
    }
    for era_id in starting_era_id.iter(count) {
        let _ = new_snapshot.insert(era_id, era_validators.clone());
    }

    new_snapshot
}

/// Generates a new `SeigniorageRecipientsSnapshot` by modifying the stakes listed in the old
/// snapshot according to the supplied list of configured accounts.
fn gen_snapshot_from_old(
    mut snapshot: SeigniorageRecipientsSnapshot,
    accounts: &[AccountConfig],
) -> SeigniorageRecipientsSnapshot {
    // Read the modifications to be applied to the validators set from the config.
    let validators_map: BTreeMap<_, _> = accounts
        .iter()
        .filter_map(|acc| {
            acc.validator
                .as_ref()
                .map(|validator| (acc.public_key.clone(), validator.clone()))
        })
        .collect();

    // We will be modifying the entries in the old snapshot passed in as `snapshot` according to
    // the config.
    for recipients in snapshot.values_mut() {
        // We use `retain` to drop some entries and modify some of the ones that will be retained.
        recipients.retain(
            |public_key, recipient| match validators_map.get(public_key) {
                // If the validator's stake is configured to be zero, we drop them from the
                // snapshot.
                Some(validator) if validator.bonded_amount.is_zero() => false,
                // Otherwise, we keep them, but modify the properties.
                Some(validator) => {
                    *recipient = SeigniorageRecipient::new(
                        validator.bonded_amount,
                        validator
                            .delegation_rate
                            // If the delegation rate wasn't specified in the config, keep the one
                            // from the old snapshot.
                            .unwrap_or(*recipient.delegation_rate()),
                        validator
                            .delegators_map()
                            // If the delegators weren't specified in the config, keep the ones
                            // from the old snapshot.
                            .unwrap_or_else(|| recipient.delegator_stake().clone()),
                    );
                    true
                }
                // Validators not present in the config will be kept unmodified.
                None => true,
            },
        );

        // Add the validators that weren't present in the old snapshot.
        for (public_key, validator) in &validators_map {
            if recipients.contains_key(public_key) {
                continue;
            }

            if validator.bonded_amount != U512::zero() {
                recipients.insert(
                    public_key.clone(),
                    SeigniorageRecipient::new(
                        validator.bonded_amount,
                        // Unspecified delegation rate will be treated as 0.
                        validator.delegation_rate.unwrap_or_default(),
                        // Unspecified delegators will be treated as an empty list.
                        validator.delegators_map().unwrap_or_default(),
                    ),
                );
            }
        }
    }

    // Return the modified snapshot.
    snapshot
}

/// Generates a set of writes necessary to "fix" the bids, ie.:
/// - set the bids of the new validators to their desired stakes,
/// - remove the bids of the old validators that are no longer validators,
/// - if `only_listed_validators` is true, remove all the bids that are larger than the smallest bid
///   among the new validators (necessary, because such bidders would outbid the validators decided
///   by the social consensus).
pub fn add_and_remove_bids<T: StateReader>(
    state: &mut StateTracker<T>,
    validators_diff: &ValidatorsDiff,
    new_snapshot: &SeigniorageRecipientsSnapshot,
    only_listed_validators: bool,
    slash: bool,
) {
    let to_unbid = if only_listed_validators {
        let large_bids = find_large_bids(state, new_snapshot);
        validators_diff
            .removed
            .union(&large_bids)
            .cloned()
            .collect()
    } else {
        validators_diff.removed.clone()
    };

    for (pub_key, seigniorage_recipient) in new_snapshot.values().rev().next().unwrap() {
        create_or_update_bid(state, pub_key, seigniorage_recipient, slash);
    }

    // Refresh the bids - we modified them above.
    let bids = state.get_bids();

    for pub_key in to_unbid {
        if let Some(bid) = bids.get(&pub_key) {
            let new_bid = Bid::empty(pub_key.clone(), *bid.bonding_purse());
            state.set_bid(pub_key.clone(), new_bid, slash);
        }
    }
}

/// Returns the set of public keys that have bids larger than the smallest bid among the new
/// validators.
fn find_large_bids<T: StateReader>(
    state: &mut StateTracker<T>,
    snapshot: &SeigniorageRecipientsSnapshot,
) -> BTreeSet<PublicKey> {
    let seigniorage_recipients = snapshot.values().next().unwrap();
    let min_bid = seigniorage_recipients
        .values()
        .map(|recipient| {
            recipient
                .total_stake()
                .expect("should have valid total stake")
        })
        .min()
        .unwrap();
    state
        .get_bids()
        .into_iter()
        .filter(|(pub_key, bid)| {
            bid.total_staked_amount()
                .map_or(true, |amount| amount >= min_bid)
                && !seigniorage_recipients.contains_key(pub_key)
        })
        .map(|(pub_key, _bid)| pub_key)
        .collect()
}

/// Updates the amount of an existing bid for the given public key, or creates a new one.
fn create_or_update_bid<T: StateReader>(
    state: &mut StateTracker<T>,
    pub_key: &PublicKey,
    recipient: &SeigniorageRecipient,
    slash: bool,
) {
    // Checks whether the data in the bid is the same as in the recipient info from a snapshot.
    let check_bid = |bid: &Bid| {
        let bid_delegators: BTreeMap<_, _> = bid
            .delegators()
            .iter()
            .map(|(key, delegator)| (key.clone(), *delegator.staked_amount()))
            .collect();
        &bid_delegators == recipient.delegator_stake() && *bid.staked_amount() == *recipient.stake()
    };

    // Check whether we need to do anything with the bid - if it exists and matches the snapshot,
    // nothing needs to be done. Otherwise we need to either create it, or change it so that it
    // matches the snapshot.
    if state
        .get_bids()
        .get(pub_key)
        .map(check_bid)
        .unwrap_or(false)
    {
        // The stake and delegators match the snapshot, nothing to do.
        return;
    }

    let stake = *recipient.stake();
    let new_bid = if let Some(old_bid) = state.get_bids().get(pub_key) {
        let mut bid = Bid::unlocked(
            pub_key.clone(),
            *old_bid.bonding_purse(),
            stake,
            *recipient.delegation_rate(),
        );

        for (delegator_pub_key, delegator_stake) in recipient.delegator_stake() {
            let delegator = if let Some(delegator) = old_bid.delegators().get(delegator_pub_key) {
                Delegator::unlocked(
                    delegator_pub_key.clone(),
                    *delegator_stake,
                    *delegator.bonding_purse(),
                    pub_key.clone(),
                )
            } else {
                let delegator_bonding_purse = state.create_purse(*delegator_stake);
                Delegator::unlocked(
                    delegator_pub_key.clone(),
                    *delegator_stake,
                    delegator_bonding_purse,
                    pub_key.clone(),
                )
            };

            bid.delegators_mut()
                .insert(delegator_pub_key.clone(), delegator);
        }

        bid
    } else {
        if stake == U512::zero() {
            // there was no bid for this key and it still is supposed to have zero amount staked -
            // nothing to do here
            return;
        }
        let bonding_purse = state.create_purse(stake);
        let mut bid = Bid::unlocked(
            pub_key.clone(),
            bonding_purse,
            stake,
            *recipient.delegation_rate(),
        );

        for (delegator_pub_key, delegator_stake) in recipient.delegator_stake() {
            let delegator_bonding_purse = state.create_purse(*delegator_stake);
            let delegator = Delegator::unlocked(
                delegator_pub_key.clone(),
                *delegator_stake,
                delegator_bonding_purse,
                pub_key.clone(),
            );

            bid.delegators_mut()
                .insert(delegator_pub_key.clone(), delegator);
        }

        bid
    };

    // This will take care of updating the bonding purse balances and unbonding purses entries.
    state.set_bid(pub_key.clone(), new_bid, slash);
}
