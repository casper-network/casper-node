use clap::ArgMatches;

use casper_types::{AsymmetricType, PublicKey, U512};

use crate::{
    generic::{
        config::{AccountConfig, Config},
        update_from_config,
    },
    utils::hash_from_str,
};

pub(crate) fn generate_validators_update(matches: &ArgMatches<'_>) {
    let data_dir = matches.value_of("data_dir").unwrap_or(".");
    let state_hash = hash_from_str(matches.value_of("hash").unwrap());
    let accounts = match matches.values_of("validator") {
        None => vec![],
        Some(values) => values
            .map(|validator_def| {
                let mut fields = validator_def.split(',').map(str::to_owned);

                let public_key_str = fields
                    .next()
                    .expect("validator config should contain a public key");
                let public_key = PublicKey::from_hex(public_key_str.as_bytes())
                    .expect("validator config should have a valid public key");

                let stake_str = fields
                    .next()
                    .expect("validator config should contain a stake");
                let stake =
                    U512::from_dec_str(&stake_str).expect("stake should be a valid decimal number");

                let maybe_new_balance_str = fields.next();
                let maybe_new_balance = maybe_new_balance_str.as_ref().map(|balance_str| {
                    U512::from_dec_str(balance_str)
                        .expect("balance should be a valid decimal number")
                });

                AccountConfig {
                    public_key,
                    stake: Some(stake),
                    balance: maybe_new_balance,
                }
            })
            .collect(),
    };

    let config = Config {
        accounts,
        transfers: vec![],
        only_listed_validators: true,
    };

    update_from_config(data_dir, state_hash, config);
}
