use casper_engine_test_support::LmdbWasmTestBuilder;
use casper_types::{
    account::Account, addressable_entity::NamedKeys, bytesrepr::ToBytes, system::mint,
    AccessRights, AsymmetricType, CLTyped, CLValue, Key, PublicKey, StoredValue, URef, U512,
};
use clap::ArgMatches;
use rand::Rng;

use crate::utils::{hash_from_str, print_entry};

const DEFAULT_MAIN_PURSE_ACCESS_RIGHTS: AccessRights = AccessRights::READ_ADD_WRITE;

fn create_purse() -> URef {
    URef::new(rand::thread_rng().gen(), DEFAULT_MAIN_PURSE_ACCESS_RIGHTS)
}

fn make_stored_clvalue<T: CLTyped + ToBytes>(value: T) -> StoredValue {
    let cl = CLValue::from_t(value).unwrap();
    StoredValue::CLValue(cl)
}

pub(crate) fn generate_admins(matches: &ArgMatches<'_>) {
    let data_dir = matches.value_of("data_dir").unwrap_or(".");
    let state_hash = matches.value_of("hash").unwrap();

    // Open the global state that should be in the supplied directory.
    let post_state_hash = hash_from_str(state_hash);
    let test_builder = LmdbWasmTestBuilder::open_raw(data_dir, Default::default(), post_state_hash);

    let admin_values = matches.values_of("admin").expect("at least one argument");

    let mut total_supply = test_builder.total_supply(Some(post_state_hash));
    let total_supply_before = total_supply;

    for value in admin_values {
        let mut fields = value.split(',').peekable();
        let field1 = fields.next().unwrap();
        let field2 = fields.next().unwrap();
        if fields.peek().is_some() {
            panic!("correct syntax for --admin parameter is [PUBLIC_KEY,BALANCE]")
        }
        let pub_key = PublicKey::from_hex(field1.as_bytes()).expect("valid public key");
        let balance = U512::from_dec_str(field2).expect("valid balance amount");

        let main_purse = create_purse();

        let purse_balance_key = Key::Balance(main_purse.addr());
        let purse_balance_value = make_stored_clvalue(balance);
        print_entry(&purse_balance_key, &purse_balance_value);

        let purse_uref_key = Key::URef(main_purse);
        let purse_uref_value = make_stored_clvalue(());
        print_entry(&purse_uref_key, &purse_uref_value);

        let account_key = Key::Account(pub_key.to_account_hash());
        let account_value = {
            let account = {
                let account_hash = pub_key.to_account_hash();
                let named_keys = NamedKeys::default();
                Account::create(account_hash, named_keys, main_purse)
            };
            StoredValue::Account(account)
        };
        print_entry(&account_key, &account_value);

        total_supply = total_supply.checked_add(balance).expect("no overflow");
    }

    if total_supply == total_supply_before {
        // Don't update total supply if it did not change
        return;
    }

    println!(
        "# total supply increases from {} to {}",
        total_supply_before, total_supply
    );

    let total_supply_key = {
        let mint_contract_hash = test_builder.get_mint_contract_hash();

        let mint_named_keys =
            test_builder.get_named_keys_by_contract_entity_hash(mint_contract_hash);

        mint_named_keys
            .get(mint::TOTAL_SUPPLY_KEY)
            .cloned()
            .expect("valid key in mint named keys")
    };
    let total_supply_value = make_stored_clvalue(total_supply);
    print_entry(&total_supply_key, &total_supply_value);
}
