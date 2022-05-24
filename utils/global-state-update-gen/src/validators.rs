mod validators_manager;

use clap::ArgMatches;

use validators_manager::ValidatorsUpdateManager;

pub(crate) fn generate_validators_update(matches: &ArgMatches<'_>) {
    let data_dir = matches.value_of("data_dir").unwrap_or(".");
    let state_hash = matches.value_of("hash").unwrap();
    let validators = match matches.values_of("validator") {
        None => vec![],
        Some(values) => values
            .map(|validator_def| {
                let mut fields = validator_def.split(',');
                let field1 = fields.next().unwrap();
                let field2 = fields.next().unwrap();
                let maybe_field3 = fields.next();
                (field1, field2, maybe_field3)
            })
            .collect(),
    };

    let mut validators_upgrade_manager = ValidatorsUpdateManager::new(data_dir, state_hash);

    validators_upgrade_manager.perform_update(validators);

    validators_upgrade_manager.print_writes();
}
