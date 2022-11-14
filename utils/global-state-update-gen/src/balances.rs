use casper_types::{account::AccountHash, U512};
use clap::ArgMatches;

use casper_engine_test_support::LmdbWasmTestBuilder;

use crate::{
    generic::{
        config::{Config, Transfer},
        update_from_config,
    },
    utils::hash_from_str,
};

pub(crate) fn generate_balances_update(matches: &ArgMatches<'_>) {
    let data_dir = matches.value_of("data_dir").unwrap_or(".");
    let state_hash = hash_from_str(matches.value_of("hash").unwrap());

    let from_account = AccountHash::from_formatted_str(matches.value_of("from").unwrap()).unwrap();
    let to_account = AccountHash::from_formatted_str(matches.value_of("to").unwrap()).unwrap();
    let amount = U512::from_str_radix(matches.value_of("amount").unwrap(), 10).unwrap();

    let config = Config {
        accounts: vec![],
        transfers: vec![Transfer {
            from: from_account,
            to: to_account,
            amount,
        }],
        only_listed_validators: false,
    };

    let builder = LmdbWasmTestBuilder::open_raw(data_dir, Default::default(), state_hash);
    update_from_config(builder, config);
}
