use clap::ArgMatches;

use casper_engine_test_support::{LmdbWasmTestBuilder, PRODUCTION_PATH};
use casper_types::{CLValue, StoredValue};

use crate::{
    auction_utils::{
        gen_snapshot, generate_entries_removing_bids, generate_entries_removing_withdraws,
        read_snapshot,
    },
    utils::{hash_from_str, print_entry, validators_diff},
};

pub(crate) fn generate_validators_update(matches: &ArgMatches<'_>) {
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

    // Open the global state that should be in the supplied directory.
    let mut test_builder =
        LmdbWasmTestBuilder::open_raw(data_dir, &*PRODUCTION_PATH, hash_from_str(state_hash));

    // Read the old SeigniorageRecipientsSnapshot
    let (validators_key, old_snapshot) = read_snapshot(&test_builder);

    // Create a new snapshot based on the old one and the supplied validators.
    let new_snapshot = gen_snapshot(
        validators,
        *old_snapshot.keys().next().unwrap(),
        old_snapshot.len() as u64,
    );

    // Print the write to the snapshot key.
    print_entry(
        &validators_key,
        &StoredValue::from(CLValue::from_t(new_snapshot.clone()).unwrap()),
    );

    let validators_diff = validators_diff(&old_snapshot, &new_snapshot);

    // Print the writes fixing the bids.
    for (key, value) in
        generate_entries_removing_bids(&mut test_builder, &validators_diff, &new_snapshot)
    {
        print_entry(&key, &value);
    }

    // Print the writes removing the no longer valid withdraws.
    for (key, value) in generate_entries_removing_withdraws(&mut test_builder, &validators_diff) {
        print_entry(&key, &value);
    }
}
