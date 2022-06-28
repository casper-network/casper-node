mod validators_manager;

use std::collections::BTreeMap;

use clap::ArgMatches;

use casper_types::{AsymmetricType, PublicKey, U512};

use validators_manager::ValidatorsUpdateManager;

pub struct ValidatorConfig {
    stake: U512,
    maybe_new_balance: Option<U512>,
}

pub(crate) fn generate_validators_update(matches: &ArgMatches<'_>) {
    let data_dir = matches.value_of("data_dir").unwrap_or(".");
    let state_hash = matches.value_of("hash").unwrap();
    let validators = match matches.values_of("validator") {
        None => BTreeMap::new(),
        Some(values) => values
            .map(|validator_def| {
                let mut fields = validator_def.split(',').map(str::to_owned);

                let pub_key_str = fields
                    .next()
                    .expect("validator config should contain a public key");
                let pub_key = PublicKey::from_hex(pub_key_str.as_bytes())
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

                (
                    pub_key,
                    ValidatorConfig {
                        stake,
                        maybe_new_balance,
                    },
                )
            })
            .collect(),
    };

    let mut validators_upgrade_manager =
        ValidatorsUpdateManager::new(data_dir, state_hash, validators);

    validators_upgrade_manager.perform_update();

    validators_upgrade_manager.print_writes();
}
