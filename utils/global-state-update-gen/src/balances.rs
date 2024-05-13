use clap::ArgMatches;

use casper_engine_test_support::LmdbWasmTestBuilder;
use casper_execution_engine::engine_state::engine_config::DEFAULT_PROTOCOL_VERSION;
use casper_types::{account::AccountHash, U512};

use crate::{
    generic::{
        config::{Config, Transfer},
        update_from_config,
    },
    utils::{hash_from_str, protocol_version_from_matches},
};

pub(crate) fn generate_balances_update(matches: &ArgMatches<'_>) {
    let data_dir = matches.value_of("data_dir").unwrap_or(".");
    let state_hash = hash_from_str(matches.value_of("hash").unwrap());

    let from_account = AccountHash::from_formatted_str(matches.value_of("from").unwrap()).unwrap();
    let to_account = AccountHash::from_formatted_str(matches.value_of("to").unwrap()).unwrap();
    let amount = U512::from_str_radix(matches.value_of("amount").unwrap(), 10).unwrap();

    let protocol_version = protocol_version_from_matches(matches);

    let config = Config {
        accounts: vec![],
        transfers: vec![Transfer {
            from: from_account,
            to: to_account,
            amount,
        }],
        only_listed_validators: false,
        slash_instead_of_unbonding: false,
        protocol_version,
    };

    let builder = LmdbWasmTestBuilder::open_raw(
        data_dir,
        Default::default(),
        DEFAULT_PROTOCOL_VERSION,
        state_hash,
    );
    update_from_config(builder, config);
}
