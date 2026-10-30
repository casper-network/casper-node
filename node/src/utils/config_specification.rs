use tracing::error;

use crate::{
    types::{EVM_TRANSACTION_BATCH_SIZE_CEILING, MIN_EVM_TRANSACTION_BATCH_SIZE},
    MainReactorConfig as Config,
};

pub fn validate_config(config: &Config) -> bool {
    if config.network.blocklist_retain_max_duration < config.network.blocklist_retain_min_duration {
        return false;
    }
    if !config.evm_transaction_batcher.is_max_batch_size_valid() {
        error!(
            max_batch_size = config.evm_transaction_batcher.max_batch_size,
            min = MIN_EVM_TRANSACTION_BATCH_SIZE,
            max = EVM_TRANSACTION_BATCH_SIZE_CEILING,
            "value for 'evm_transaction_batcher.max_batch_size' is outside the supported range",
        );
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::validate_config;
    use crate::{
        types::{EVM_TRANSACTION_BATCH_SIZE_CEILING, MIN_EVM_TRANSACTION_BATCH_SIZE},
        MainReactorConfig as Config,
    };
    use casper_types::TimeDiff;

    #[test]
    fn validate_config_should_fail_malformed_blocklist_definition() {
        let mut config = Config::default();
        config.network.blocklist_retain_max_duration = TimeDiff::from_seconds(10);
        config.network.blocklist_retain_min_duration = TimeDiff::from_seconds(11);
        assert!(!validate_config(&config));
    }

    #[test]
    fn validate_config_should_not_fail_when_blocklist_definitions_are_ok() {
        let mut config = Config::default();
        config.network.blocklist_retain_max_duration = TimeDiff::from_seconds(11);
        config.network.blocklist_retain_min_duration = TimeDiff::from_seconds(10);
        assert!(validate_config(&config));
        config.network.blocklist_retain_max_duration = TimeDiff::from_seconds(10);
        config.network.blocklist_retain_min_duration = TimeDiff::from_seconds(10);
        assert!(validate_config(&config));
    }

    #[test]
    fn validate_config_should_fail_out_of_range_evm_max_batch_size() {
        let mut config = Config::default();
        for max_batch_size in [
            0,
            MIN_EVM_TRANSACTION_BATCH_SIZE - 1,
            EVM_TRANSACTION_BATCH_SIZE_CEILING + 1,
        ] {
            config.evm_transaction_batcher.max_batch_size = max_batch_size as u32;
            assert!(
                !validate_config(&config),
                "{max_batch_size} should be rejected"
            );
        }
        for max_batch_size in [
            MIN_EVM_TRANSACTION_BATCH_SIZE,
            10,
            EVM_TRANSACTION_BATCH_SIZE_CEILING,
        ] {
            config.evm_transaction_batcher.max_batch_size = max_batch_size as u32;
            assert!(
                validate_config(&config),
                "{max_batch_size} should be accepted"
            );
        }
    }
}
