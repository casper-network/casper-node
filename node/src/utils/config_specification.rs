use tracing::error;

use crate::MainReactorConfig as Config;

/// We don't allow flakiness to be turned on in mainnet and testnet
const NETWORK_NAMES_NOT_ALLOWING_FLAKINESS: &[&str] = &["casper", "casper-test"];

pub fn validate_config(network_name: &str, config: &Config) -> bool {
    if config.network.blocklist_retain_max_duration < config.network.blocklist_retain_min_duration {
        return false;
    }
    if let Some(flakiness) = &config.network.flakiness {
        if flakiness.block_peer_after_drop_max < flakiness.block_peer_after_drop_min {
            return false;
        }
        if flakiness.drop_peer_after_max < flakiness.drop_peer_after_min {
            return false;
        }
    }

    if config.network.flakiness.is_some()
        && NETWORK_NAMES_NOT_ALLOWING_FLAKINESS
            .iter()
            .any(|el| *el == network_name)
    {
        error!("Flakiness config not allowed in network: {network_name}");
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::validate_config;
    use crate::{components::network::NetworkFlakinessConfig, MainReactorConfig as Config};
    use casper_types::TimeDiff;

    #[test]
    fn validate_config_should_fail_malformed_blocklist_definition() {
        let mut config = Config::default();
        config.network.blocklist_retain_max_duration = TimeDiff::from_seconds(10);
        config.network.blocklist_retain_min_duration = TimeDiff::from_seconds(11);
        assert!(!validate_config("x", &config));
    }

    #[test]
    fn validate_config_should_not_fail_when_blocklist_definitions_are_ok() {
        let mut config = Config::default();
        config.network.blocklist_retain_max_duration = TimeDiff::from_seconds(11);
        config.network.blocklist_retain_min_duration = TimeDiff::from_seconds(10);
        assert!(validate_config("x", &config));
        config.network.blocklist_retain_max_duration = TimeDiff::from_seconds(10);
        config.network.blocklist_retain_min_duration = TimeDiff::from_seconds(10);
        assert!(validate_config("x", &config));
    }

    #[test]
    fn validate_config_should_not_allow_flakiness_with_some_networks() {
        let mut config = Config::default();
        config.network.flakiness = Some(NetworkFlakinessConfig::default());

        assert!(validate_config("x", &config));
        assert!(!validate_config("casper", &config));
        assert!(!validate_config("casper-test", &config));
    }

    #[test]
    fn validate_config_should_allow_all_networks_when_no_flakiness() {
        let mut config = Config::default();
        config.network.flakiness = None;

        assert!(validate_config("x", &config));
        assert!(validate_config("casper", &config));
        assert!(validate_config("casper-test", &config));
    }
}
