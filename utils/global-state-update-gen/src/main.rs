use std::{
    collections::{BTreeMap, BTreeSet},
    convert::TryInto,
};

use casper_engine_test_support::internal::LmdbWasmTestBuilder;
use casper_execution_engine::shared::{newtypes::Blake2bHash, stored_value::StoredValue};
use casper_types::{
    bytesrepr::ToBytes,
    system::auction::{Bid, SeigniorageRecipient, SeigniorageRecipientsSnapshot},
    AsymmetricType, CLValue, Key, ProtocolVersion, PublicKey, U512,
};

use clap::{App, Arg};

fn hash_from_str(hex_str: &str) -> Blake2bHash {
    (&base16::decode(hex_str).unwrap()[..]).try_into().unwrap()
}

fn gen_snapshot(
    validators: Vec<(String, String)>,
    starting_era_id: u64,
    count: u64,
) -> SeigniorageRecipientsSnapshot {
    let mut new_snapshot = BTreeMap::new();
    for era_id in starting_era_id..starting_era_id + count {
        let mut era_validators = BTreeMap::new();
        for (key_str, bonded_str) in &validators {
            let key = PublicKey::from_hex(key_str.as_bytes()).unwrap();
            let bonded_amount = U512::from_dec_str(bonded_str).unwrap();
            let seigniorage_recipient =
                SeigniorageRecipient::new(bonded_amount, Default::default(), Default::default());
            let _ = era_validators.insert(key, seigniorage_recipient);
        }
        let _ = new_snapshot.insert(era_id, era_validators);
    }

    new_snapshot
}

fn find_large_bids(
    builder: &mut LmdbWasmTestBuilder,
    new_snapshot: &SeigniorageRecipientsSnapshot,
) -> BTreeSet<PublicKey> {
    let max_bid = new_snapshot
        .values()
        .next()
        .unwrap()
        .values()
        .map(SeigniorageRecipient::stake)
        .max()
        .unwrap();
    builder
        .get_bids()
        .into_iter()
        .filter(|(_pkey, bid)| bid.staked_amount() >= max_bid)
        .map(|(pkey, _bid)| pkey)
        .collect()
}

struct ValidatorsDiff {
    added: BTreeSet<PublicKey>,
    removed: BTreeSet<PublicKey>,
}

fn validators_diff(
    old_snapshot: &SeigniorageRecipientsSnapshot,
    new_snapshot: &SeigniorageRecipientsSnapshot,
) -> ValidatorsDiff {
    let old_validators: BTreeSet<_> = old_snapshot
        .values()
        .flat_map(BTreeMap::keys)
        .cloned()
        .collect();
    let new_validators: BTreeSet<_> = new_snapshot
        .values()
        .flat_map(BTreeMap::keys)
        .cloned()
        .collect();

    ValidatorsDiff {
        added: new_validators
            .difference(&old_validators)
            .cloned()
            .collect(),
        removed: old_validators
            .difference(&new_validators)
            .cloned()
            .collect(),
    }
}

fn fix_bids(
    builder: &mut LmdbWasmTestBuilder,
    validators_diff: &ValidatorsDiff,
    new_snapshot: &SeigniorageRecipientsSnapshot,
) -> BTreeMap<Key, StoredValue> {
    let large_bids = find_large_bids(builder, new_snapshot);
    let to_unbid = validators_diff.removed.union(&large_bids);

    validators_diff
        .added
        .iter()
        .map(|pkey| {
            let amount = *new_snapshot
                .values()
                .next()
                .unwrap()
                .get(pkey)
                .unwrap()
                .stake();
            let account_hash = pkey.to_account_hash();
            let account = builder.get_account(account_hash).unwrap();
            (
                Key::Bid(account_hash),
                Bid::unlocked(*pkey, account.main_purse(), amount, Default::default()).into(),
            )
        })
        .chain(to_unbid.into_iter().map(|pkey| {
            let account_hash = pkey.to_account_hash();
            let account = builder.get_account(account_hash).unwrap();
            (
                Key::Bid(account_hash),
                Bid::empty(*pkey, account.main_purse()).into(),
            )
        }))
        .collect()
}

fn fix_withdraws(
    builder: &mut LmdbWasmTestBuilder,
    validators_diff: &ValidatorsDiff,
) -> BTreeMap<Key, StoredValue> {
    let withdraws = builder.get_withdraws();
    let withdraw_keys: BTreeSet<_> = withdraws.keys().collect();
    validators_diff
        .removed
        .iter()
        .map(PublicKey::to_account_hash)
        .filter(|acc| withdraw_keys.contains(&acc))
        .map(|acc| (Key::Withdraw(acc), StoredValue::Withdraw(vec![])))
        .collect()
}

fn print_entry(key: &Key, value: &StoredValue) {
    println!("[[entries]]");
    println!("key = \"{}\"", key.to_formatted_string());
    println!("value = \"{}\"", base64::encode(value.to_bytes().unwrap()));
    println!();
}

fn main() {
    let matches = App::new("Global State Update Generator")
        .version("0.1")
        .about("Generates a global state update file based on the supplied parameters")
        .arg(
            Arg::with_name("data_dir")
                .short("d")
                .long("data-dir")
                .value_name("PATH")
                .help("Data storage directory containing the global state database file")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("hash")
                .short("h")
                .long("hash")
                .value_name("HASH")
                .help("The global state hash to be used as the base")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name("validator")
                .short("v")
                .long("validator")
                .value_name("KEY,STAKE")
                .help("A new validator with their stake")
                .takes_value(true)
                .required(true)
                .multiple(true)
                .number_of_values(1),
        )
        .get_matches();

    let data_dir = matches.value_of("data_dir").unwrap_or(".");
    let state_hash = matches.value_of("hash").unwrap();
    let validators = match matches.values_of("validator") {
        None => vec![],
        Some(values) => values
            .map(|validator_def| {
                let mut fields = validator_def.split(',').map(str::to_owned);
                let field1 = fields.next().unwrap();
                let field2 = fields.next().unwrap();
                (field1, field2)
            })
            .collect(),
    };

    let mut test_builder =
        LmdbWasmTestBuilder::open(data_dir, Default::default(), hash_from_str(state_hash));

    let protocol_data = test_builder
        .get_engine_state()
        .get_protocol_data(ProtocolVersion::from_parts(1, 0, 0))
        .unwrap()
        .expect("should have protocol data");

    let auction_contract_hash = protocol_data.auction();

    let validators_key = test_builder
        .get_contract(auction_contract_hash)
        .expect("auction should exist")
        .named_keys()["seigniorage_recipients_snapshot"];

    let stored_value = test_builder
        .query(None, validators_key, &[])
        .expect("should query");
    let cl_value = stored_value
        .as_cl_value()
        .cloned()
        .expect("should be cl value");
    let old_snapshot: SeigniorageRecipientsSnapshot = cl_value.into_t().expect("should convert");

    let new_snapshot = gen_snapshot(
        validators,
        *old_snapshot.keys().next().unwrap(),
        old_snapshot.len() as u64,
    );

    print_entry(
        &validators_key,
        &StoredValue::from(CLValue::from_t(new_snapshot.clone()).unwrap()),
    );

    let validators_diff = validators_diff(&old_snapshot, &new_snapshot);

    for (key, value) in fix_bids(&mut test_builder, &validators_diff, &new_snapshot) {
        print_entry(&key, &value);
    }

    for (key, value) in fix_withdraws(&mut test_builder, &validators_diff) {
        print_entry(&key, &value);
    }
}
