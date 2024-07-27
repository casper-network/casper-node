use std::collections::BTreeMap;

use casper_engine_test_support::LmdbWasmTestBuilder;
use casper_types::{CLValue, Key, StoredValue, U512};
use clap::ArgMatches;
use csv::StringRecord;

use crate::{generic::update::Update, utils::hash_from_str};

pub(crate) fn generate_emergency_balances_update(matches: &ArgMatches<'_>) {
    let data_dir = matches.value_of("data_dir").unwrap_or(".");
    let state_hash = hash_from_str(matches.value_of("hash").unwrap());
    let dest = Key::from_formatted_str(matches.value_of("dest").unwrap()).unwrap();
    assert!(
        matches!(dest, Key::Account(_) | Key::URef(_)),
        "Destination must be an account or uref"
    );

    let file = matches.value_of("file").unwrap();

    let mut csv_reader = csv::Reader::from_path(file).unwrap();

    if csv_reader.headers().unwrap() != &StringRecord::from(vec!["key", "amount"]) {
        panic!("First record in CSV file must be the header key,amount");
    }

    let records: Vec<_> = csv_reader.records().map(|result| result.unwrap()).collect();
    if records.is_empty() {
        panic!("No records found in CSV file");
    }

    eprintln!("Found {} records", records.len());

    let builder = LmdbWasmTestBuilder::open_raw(data_dir, Default::default(), state_hash);

    let mut entries = BTreeMap::new();

    let mut total_amounts = U512::zero();

    for record in records {
        let mut fields = record.iter();
        let key = fields.next().unwrap();
        let amount = fields.next().unwrap();

        let amount = U512::from_str_radix(amount, 10).unwrap();

        let key = Key::from_formatted_str(key).unwrap_or_else(|error| {
            panic!("Unable to parse key {key}: {error}");
        });

        let balance_uref = match key {
            Key::Account(account_hash) => {
                let account = builder.get_account(account_hash).unwrap();
                account.main_purse()
            }
            Key::URef(uref) => uref,
            other => panic!("Unexpected key type: {:?}", other),
        };

        let current_balance = builder.get_purse_balance(balance_uref);
        assert!(
            current_balance >= amount,
            "Expected balance to be at least {}, but found {}",
            amount,
            current_balance
        );

        let new_balance = current_balance.checked_sub(amount).unwrap();
        total_amounts = total_amounts.checked_add(amount).unwrap();

        entries.insert(
            Key::Balance(balance_uref.addr()),
            StoredValue::CLValue(CLValue::from_t(new_balance).unwrap()),
        );
    }

    let dest_uref = match dest {
        Key::Account(account_hash) => {
            let account = builder.get_account(account_hash).unwrap();
            account.main_purse()
        }
        Key::URef(uref) => uref,
        _ => unreachable!(),
    };

    let mut dest_purse_balance = builder.get_purse_balance(dest_uref);
    eprintln!("Dest purse balance: {}", dest_purse_balance);
    eprintln!("Adding {} to dest purse {dest_uref:?}", total_amounts);
    dest_purse_balance = dest_purse_balance.checked_add(total_amounts).unwrap();
    entries.insert(
        Key::Balance(dest_uref.addr()),
        StoredValue::CLValue(CLValue::from_t(dest_purse_balance).unwrap()),
    );

    let update = Update::new(entries, None);
    update.print();
}
