use crate::MainReactorConfig as Config;

pub fn validate_config(config: &Config) -> bool {
    if config.network.blocklist_retain_max_duration < config.network.blocklist_retain_min_duration {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::validate_config;
    use crate::MainReactorConfig as Config;
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
}
