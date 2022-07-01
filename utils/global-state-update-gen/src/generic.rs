mod config;
mod state_tracker;

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};

use casper_hashing::Digest;
use casper_types::{
    system::auction::{Bid, SeigniorageRecipient, SeigniorageRecipientsSnapshot},
    CLValue, EraId, PublicKey, StoredValue, U512,
};

use clap::ArgMatches;

use crate::utils::{hash_from_str, validators_diff, ValidatorsDiff};

use self::{
    config::{AccountConfig, Config, Transfer},
    state_tracker::StateTracker,
};

pub(crate) fn generate_generic_update(matches: &ArgMatches<'_>) {
    let data_dir = matches.value_of("data_dir").unwrap_or(".");
    let state_hash = hash_from_str(matches.value_of("hash").unwrap());
    let config_path = matches.value_of("config_file").unwrap();

    let config_bytes = fs::read(config_path).expect("couldn't read the config file");
    let config: Config = toml::from_slice(&config_bytes).expect("couldn't parse the config file");

    update_from_config(data_dir, state_hash, config);
}

pub(crate) fn update_from_config(data_dir: &str, state_hash: Digest, config: Config) {
    let mut state_tracker = StateTracker::new(data_dir, state_hash);

    process_transfers(&mut state_tracker, &config.transfers);

    update_account_balances(&mut state_tracker, &config.accounts);

    update_auction_state(
        &mut state_tracker,
        &config.accounts,
        config.only_listed_validators,
    );

    state_tracker.print_all_entries();
}

fn process_transfers(state: &mut StateTracker, transfers: &[Transfer]) {
    for transfer in transfers {
        state.execute_transfer(transfer);
    }
}

fn update_account_balances(state: &mut StateTracker, accounts: &[AccountConfig]) {
    for account in accounts {
        let target_balance = if let Some(balance) = account.balance {
            balance
        } else {
            continue;
        };
        if let Some(account) = state.get_account(&account.public_key) {
            state.set_purse_balance(account.main_purse(), target_balance);
        } else {
            state.create_account(account.public_key.clone(), target_balance);
        }
    }
}

fn update_auction_state(
    state: &mut StateTracker,
    accounts: &[AccountConfig],
    only_listed_validators: bool,
) {
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
    );

    state.remove_withdraws(&validators_diff.removed);
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
        // don't add validatord with zero stake to the snapshot
        let stake = match account.stake {
            Some(stake) if stake != U512::zero() => stake,
            _ => continue,
        };
        let seigniorage_recipient =
            SeigniorageRecipient::new(stake, Default::default(), Default::default());
        let _ = era_validators.insert(account.public_key.clone(), seigniorage_recipient);
    }
    for era_id in starting_era_id.iter(count) {
        let _ = new_snapshot.insert(era_id, era_validators.clone());
    }

    new_snapshot
}

/// Generates a new `SeigniorageRecipientsSnapshot` by modifying the stakes listed in the old
/// snaphot according to the supplied list of configured accounts.
fn gen_snapshot_from_old(
    mut snapshot: SeigniorageRecipientsSnapshot,
    accounts: &[AccountConfig],
) -> SeigniorageRecipientsSnapshot {
    let stakes_map: BTreeMap<_, _> = accounts
        .iter()
        .filter_map(|acc| acc.stake.map(|stake| (acc.public_key.clone(), stake)))
        .collect();

    for recipients in snapshot.values_mut() {
        let mut to_drop = vec![];
        for (public_key, recipient) in recipients.iter_mut() {
            match stakes_map.get(public_key) {
                Some(stake) if *stake == U512::zero() => {
                    to_drop.push(public_key.clone());
                }
                Some(stake) => {
                    *recipient =
                        SeigniorageRecipient::new(*stake, Default::default(), Default::default());
                }
                None => (),
            }
        }
        for public_key in to_drop {
            recipients.remove(&public_key);
        }
    }

    snapshot
}

/// Generates a set of writes necessary to "fix" the bids, ie.:
/// - set the bids of the new validators to their desired stakes,
/// - remove the bids of the old validators that are no longer validators,
/// - remove all the bids that are larger than the smallest bid among the new validators
/// (necessary, because such bidders would outbid the validators decided by the social
/// consensus).
pub fn add_and_remove_bids(
    state: &mut StateTracker,
    validators_diff: &ValidatorsDiff,
    new_snapshot: &SeigniorageRecipientsSnapshot,
    only_listed_validators: bool,
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

    for (pub_key, seigniorage_recipient) in new_snapshot.values().next().unwrap() {
        let stake = *seigniorage_recipient.stake();
        create_or_update_bid(state, pub_key, stake);
    }

    // Refresh the bids - we modified them above.
    let bids = state.get_bids();

    for pub_key in to_unbid {
        if let Some(bid) = bids.get(&pub_key) {
            let new_bid = Bid::empty(pub_key.clone(), *bid.bonding_purse());
            state.set_bid(pub_key.clone(), new_bid);
        }
    }
}

/// Returns the set of public keys that have bids larger than the smallest bid among the new
/// validators.
fn find_large_bids(
    state: &mut StateTracker,
    snapshot: &SeigniorageRecipientsSnapshot,
) -> BTreeSet<PublicKey> {
    let seigniorage_recipients = snapshot.values().next().unwrap();
    let min_bid = seigniorage_recipients
        .values()
        .map(SeigniorageRecipient::stake)
        .min()
        .unwrap();
    state
        .get_bids()
        .into_iter()
        .filter(|(pub_key, bid)| {
            bid.total_staked_amount()
                .map_or(true, |amount| amount >= *min_bid)
                && !seigniorage_recipients.contains_key(pub_key)
        })
        .map(|(pub_key, _bid)| pub_key)
        .collect()
}

/// Updates the amount of an existing bid for the given public key, or creates a new one.
fn create_or_update_bid(state: &mut StateTracker, pub_key: &PublicKey, stake: U512) {
    let new_bid = if let Some(bid) = state.get_bids().get(pub_key) {
        Bid::unlocked(
            pub_key.clone(),
            *bid.bonding_purse(),
            stake,
            Default::default(),
        )
    } else {
        if stake == U512::zero() {
            // there was no bid for this key and it still is supposed to have zero amount staked -
            // nothing to do here
            return;
        }
        let bonding_purse = state.create_purse(stake);
        Bid::unlocked(pub_key.clone(), bonding_purse, stake, Default::default())
    };
    state.set_bid(pub_key.clone(), new_bid);
}
