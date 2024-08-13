use std::collections::BTreeMap;

use casper_engine_test_support::LmdbWasmTestBuilder;
use casper_types::{account::Account, CLValue, Key, StoredValue, U512};
use clap::ArgMatches;
use csv::StringRecord;

use crate::{generic::update::Update, utils::hash_from_str};

#[derive(Debug, PartialEq)]
enum Action {
    SubtractBalance(Key, U512),
    PurgeContractPackageHash(Key),
    RemoveNamedKey(Key, String),
}

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

    if csv_reader.headers().unwrap() != &StringRecord::from(vec!["key", "action", "payload"]) {
        panic!("First record in CSV file must be the header key,action,payload");
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
        let action = fields.next().unwrap();
        let payload = fields.next().unwrap();

        let action = match action {
            "subtract_balance" => {
                let key = Key::from_formatted_str(key).unwrap_or_else(|error| {
                    panic!("Unable to parse key {key}: {error}");
                });
                Action::SubtractBalance(key, U512::from_str_radix(payload, 10).unwrap())
            }
            "purge_contract_package_hash" => {
                let key = Key::from_formatted_str(key).unwrap_or_else(|error| {
                    panic!("Unable to parse key {key}: {error}");
                });
                assert!(
                    matches!(key, Key::Hash(_)),
                    "Expected contract package hash key"
                );
                assert!(
                    payload.is_empty(),
                    "Expected empty payload for purge_contract_package_hash"
                );
                Action::PurgeContractPackageHash(key)
            }
            "remove_named_key" => {
                let key = Key::from_formatted_str(key).unwrap_or_else(|error| {
                    panic!("Unable to parse account hash {key}: {error}");
                });
                assert!(matches!(key, Key::Account(_)), "Expected account key");
                Action::RemoveNamedKey(key, payload.to_string())
            }
            other => panic!("Unknown action: {}", other),
        };

        eprintln!("Action {action:?}");

        match action {
            Action::SubtractBalance(source_key, amount) => {
                let balance_uref = match source_key {
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
            Action::PurgeContractPackageHash(contract_package_hash_key) => {
                eprintln!(
                    "Found a hash key {contract_package_hash_key:?}, inserting a null CLValue"
                );

                let _contract_package = match builder.query(None, contract_package_hash_key, &[]) {
                    Ok(StoredValue::ContractPackage(contract_package)) => contract_package,
                    Ok(other) => panic!("Unexpected value for contract package: {:?}", other),
                    Err(error) => {
                        panic!(
                            "Can't find contract package at {contract_package_hash_key:?}: {error}"
                        );
                    }
                };

                let value = StoredValue::CLValue(CLValue::unit());
                entries.insert(contract_package_hash_key, value);
            }
            Action::RemoveNamedKey(account_hash_key, named_key) => {
                let account_hash = account_hash_key
                    .as_account()
                    .expect("Expected account hash");
                let old_account = builder
                    .get_account(*account_hash)
                    .expect("Account not found");

                let mut new_named_keys = old_account.named_keys().clone();
                let old_named_key = new_named_keys.remove(&named_key);
                assert!(
                    old_named_key.is_some(),
                    "Named key {named_key} not found in account {account_hash:?}"
                );

                let new_account = Account::new(
                    old_account.account_hash(),
                    new_named_keys,
                    old_account.main_purse(),
                    old_account.associated_keys().clone(),
                    old_account.action_thresholds().clone(),
                );

                assert!(
                    matches!(account_hash_key, Key::Account(_)),
                    "Expected account key"
                );

                entries.insert(account_hash_key, StoredValue::Account(new_account));
            }
        }
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
