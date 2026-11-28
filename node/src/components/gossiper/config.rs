use std::str::FromStr;

use datasize::DataSize;
use serde::{
    de::{Deserializer, Error as SerdeError, Unexpected},
    Deserialize, Serialize,
};
use tracing::error;

use casper_types::TimeDiff;

#[cfg(test)]
use super::error::Error;

const DEFAULT_INFECTION_TARGET: u8 = 3;
const DEFAULT_SATURATION_LIMIT_PERCENT: u8 = 80;
const DEFAULT_NEXT_ROUND_SUCCESS_THRESHOLD: u8 = DEFAULT_INFECTION_TARGET;
const DEFAULT_MAX_ROUNDS_ADDRESS: u8 = 1;
const DEFAULT_MAX_ROUNDS_TRANSACTION: u8 = 3;
const DEFAULT_MAX_ROUNDS_EVM_TRANSACTION_BATCH: u8 = 3;
const DEFAULT_MAX_ROUNDS_BLOCK: u8 = 3;
const DEFAULT_MAX_ROUNDS_FINALITY_SIGNATURE: u8 = 2;
pub(super) const MAX_SATURATION_LIMIT_PERCENT: u8 = 99;
pub(super) const DEFAULT_FINISHED_ENTRY_DURATION: &str = "60sec";
const DEFAULT_GOSSIP_REQUEST_TIMEOUT: &str = "10sec";
const DEFAULT_GET_REMAINDER_TIMEOUT: &str = "60sec";
const DEFAULT_VALIDATE_AND_STORE_TIMEOUT: &str = "60sec";
#[cfg(test)]
const SMALL_TIMEOUTS_FINISHED_ENTRY_DURATION: &str = "2sec";
#[cfg(test)]
const SMALL_TIMEOUTS_GOSSIP_REQUEST_TIMEOUT: &str = "1sec";
#[cfg(test)]
const SMALL_TIMEOUTS_GET_REMAINDER_TIMEOUT: &str = "1sec";
#[cfg(test)]
const SMALL_TIMEOUTS_VALIDATE_AND_STORE_TIMEOUT: &str = "1sec";

/// Configuration options for gossiping.
#[derive(Copy, Clone, DataSize, Debug, Deserialize, Serialize)]
pub struct Config {
    /// Target number of peers to infect with a given piece of data.
    pub infection_target: u8,
    /// The saturation limit as a percentage, with a maximum value of 99.  Used as a termination
    /// condition.
    ///
    /// Example: assume the `infection_target` is 3, the `saturation_limit_percent` is 80, and we
    /// don't manage to newly infect 3 peers.  We will stop gossiping once we know of more than 15
    /// holders excluding us since 80% saturation would imply 3 new infections in 15 peers.
    #[serde(deserialize_with = "deserialize_saturation_limit_percent")]
    pub saturation_limit_percent: u8,
    /// The maximum duration in seconds for which to keep finished entries.
    ///
    /// The longer they are retained, the lower the likelihood of re-gossiping a piece of data.
    /// However, the longer they are retained, the larger the list of finished entries can grow.
    pub finished_entry_duration: TimeDiff,
    /// The timeout duration in seconds for a single gossip request, i.e. for a single gossip
    /// message sent from this node, it will be considered timed out if the expected response from
    /// that peer is not received within this specified duration.
    pub gossip_request_timeout: TimeDiff,
    /// The timeout duration in seconds for retrieving the remaining part(s) of newly-discovered
    /// data from a peer which gossiped information about that data to this node.
    pub get_remainder_timeout: TimeDiff,
    /// The timeout duration for a newly-received, gossiped item to be validated and stored by
    /// another component before the gossiper abandons waiting to gossip the item onwards.
    pub validate_and_store_timeout: TimeDiff,
    /// The number of peers newly infected by us during a gossip round at or above which we start
    /// another round, targeting peers not attempted in any earlier round.
    ///
    /// A round ends once `infection_target` peers have been newly infected in it, or once the
    /// saturation limit has been reached for the peers attempted in it.  A value greater than
    /// `infection_target` effectively disables additional rounds.
    #[serde(default = "default_next_round_success_threshold")]
    pub next_round_success_threshold: u8,
    /// The maximum number of gossip rounds per item, configured per kind of gossiped item.
    #[serde(default)]
    pub max_rounds: MaxGossipRounds,
}

/// The maximum number of gossip rounds for each kind of gossiped item.
///
/// A value of 1 (or 0, which is treated as 1) means no additional rounds will be started.
#[derive(Copy, Clone, DataSize, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct MaxGossipRounds {
    /// Maximum number of rounds for gossiping peer addresses.
    pub address: u8,
    /// Maximum number of rounds for gossiping transactions.
    pub transaction: u8,
    /// Maximum number of rounds for gossiping EVM transaction batches.
    pub evm_transaction_batch: u8,
    /// Maximum number of rounds for gossiping blocks.
    pub block: u8,
    /// Maximum number of rounds for gossiping finality signatures.
    pub finality_signature: u8,
}

impl Default for MaxGossipRounds {
    fn default() -> Self {
        MaxGossipRounds {
            address: DEFAULT_MAX_ROUNDS_ADDRESS,
            transaction: DEFAULT_MAX_ROUNDS_TRANSACTION,
            evm_transaction_batch: DEFAULT_MAX_ROUNDS_EVM_TRANSACTION_BATCH,
            block: DEFAULT_MAX_ROUNDS_BLOCK,
            finality_signature: DEFAULT_MAX_ROUNDS_FINALITY_SIGNATURE,
        }
    }
}

fn default_next_round_success_threshold() -> u8 {
    DEFAULT_NEXT_ROUND_SUCCESS_THRESHOLD
}

impl Config {
    #[cfg(test)]
    pub(crate) fn new(
        infection_target: u8,
        saturation_limit_percent: u8,
        finished_entry_duration: TimeDiff,
        gossip_request_timeout: TimeDiff,
        get_remainder_timeout: TimeDiff,
        validate_and_store_timeout: TimeDiff,
    ) -> Result<Self, Error> {
        if saturation_limit_percent > MAX_SATURATION_LIMIT_PERCENT {
            return Err(Error::InvalidSaturationLimit);
        }
        Ok(Config {
            infection_target,
            saturation_limit_percent,
            finished_entry_duration,
            gossip_request_timeout,
            get_remainder_timeout,
            validate_and_store_timeout,
            next_round_success_threshold: DEFAULT_NEXT_ROUND_SUCCESS_THRESHOLD,
            max_rounds: MaxGossipRounds::default(),
        })
    }

    #[cfg(test)]
    pub(crate) fn new_with_small_timeouts() -> Self {
        Config {
            finished_entry_duration: TimeDiff::from_str(SMALL_TIMEOUTS_FINISHED_ENTRY_DURATION)
                .unwrap(),
            gossip_request_timeout: TimeDiff::from_str(SMALL_TIMEOUTS_GOSSIP_REQUEST_TIMEOUT)
                .unwrap(),
            get_remainder_timeout: TimeDiff::from_str(SMALL_TIMEOUTS_GET_REMAINDER_TIMEOUT)
                .unwrap(),
            validate_and_store_timeout: TimeDiff::from_str(
                SMALL_TIMEOUTS_VALIDATE_AND_STORE_TIMEOUT,
            )
            .unwrap(),
            ..Default::default()
        }
    }

    pub(crate) fn infection_target(&self) -> u8 {
        self.infection_target
    }

    pub(crate) fn saturation_limit_percent(&self) -> u8 {
        self.saturation_limit_percent
    }

    pub(crate) fn finished_entry_duration(&self) -> TimeDiff {
        self.finished_entry_duration
    }

    pub(crate) fn gossip_request_timeout(&self) -> TimeDiff {
        self.gossip_request_timeout
    }

    pub(crate) fn get_remainder_timeout(&self) -> TimeDiff {
        self.get_remainder_timeout
    }

    pub(crate) fn validate_and_store_timeout(&self) -> TimeDiff {
        self.validate_and_store_timeout
    }

    pub(crate) fn next_round_success_threshold(&self) -> u8 {
        self.next_round_success_threshold
    }

    pub(crate) fn max_rounds(&self) -> MaxGossipRounds {
        self.max_rounds
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            infection_target: DEFAULT_INFECTION_TARGET,
            saturation_limit_percent: DEFAULT_SATURATION_LIMIT_PERCENT,
            finished_entry_duration: TimeDiff::from_str(DEFAULT_FINISHED_ENTRY_DURATION).unwrap(),
            gossip_request_timeout: TimeDiff::from_str(DEFAULT_GOSSIP_REQUEST_TIMEOUT).unwrap(),
            get_remainder_timeout: TimeDiff::from_str(DEFAULT_GET_REMAINDER_TIMEOUT).unwrap(),
            validate_and_store_timeout: TimeDiff::from_str(DEFAULT_VALIDATE_AND_STORE_TIMEOUT)
                .unwrap(),
            next_round_success_threshold: DEFAULT_NEXT_ROUND_SUCCESS_THRESHOLD,
            max_rounds: MaxGossipRounds::default(),
        }
    }
}

/// Deserializes a `usize` but fails if it's not in the range 0..100.
fn deserialize_saturation_limit_percent<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: Deserializer<'de>,
{
    let saturation_limit_percent = u8::deserialize(deserializer)?;
    if saturation_limit_percent > MAX_SATURATION_LIMIT_PERCENT {
        error!(
            "saturation_limit_percent of {} is above {}",
            saturation_limit_percent, MAX_SATURATION_LIMIT_PERCENT
        );
        return Err(SerdeError::invalid_value(
            Unexpected::Unsigned(saturation_limit_percent as u64),
            &"a value between 0 and 99 inclusive",
        ));
    }

    Ok(saturation_limit_percent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_config_should_fail() {
        // saturation_limit_percent > MAX_SATURATION_LIMIT_PERCENT
        let invalid_config = Config {
            infection_target: 3,
            saturation_limit_percent: MAX_SATURATION_LIMIT_PERCENT + 1,
            finished_entry_duration: TimeDiff::from_str(DEFAULT_FINISHED_ENTRY_DURATION).unwrap(),
            gossip_request_timeout: TimeDiff::from_str(DEFAULT_GOSSIP_REQUEST_TIMEOUT).unwrap(),
            get_remainder_timeout: TimeDiff::from_str(DEFAULT_GET_REMAINDER_TIMEOUT).unwrap(),
            validate_and_store_timeout: TimeDiff::from_str(DEFAULT_VALIDATE_AND_STORE_TIMEOUT)
                .unwrap(),
            next_round_success_threshold: DEFAULT_NEXT_ROUND_SUCCESS_THRESHOLD,
            max_rounds: MaxGossipRounds::default(),
        };

        // Parsing should fail.
        let config_as_json = serde_json::to_string(&invalid_config).unwrap();
        assert!(serde_json::from_str::<Config>(&config_as_json).is_err());

        // Construction should fail.
        assert!(Config::new(
            3,
            MAX_SATURATION_LIMIT_PERCENT + 1,
            TimeDiff::from_str(DEFAULT_FINISHED_ENTRY_DURATION).unwrap(),
            TimeDiff::from_str(DEFAULT_GOSSIP_REQUEST_TIMEOUT).unwrap(),
            TimeDiff::from_str(DEFAULT_GET_REMAINDER_TIMEOUT).unwrap(),
            TimeDiff::from_str(DEFAULT_VALIDATE_AND_STORE_TIMEOUT).unwrap()
        )
        .is_err())
    }

    #[test]
    fn config_without_round_settings_should_use_defaults() {
        let toml = r#"
            infection_target = 3
            saturation_limit_percent = 80
            finished_entry_duration = '1 minute'
            gossip_request_timeout = '10 seconds'
            get_remainder_timeout = '5 seconds'
            validate_and_store_timeout = '1 minute'
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(
            config.next_round_success_threshold,
            DEFAULT_NEXT_ROUND_SUCCESS_THRESHOLD
        );
        assert_eq!(config.max_rounds, MaxGossipRounds::default());
    }

    #[test]
    fn partial_max_rounds_should_use_defaults_for_missing_kinds() {
        let toml = r#"
            infection_target = 3
            saturation_limit_percent = 80
            finished_entry_duration = '1 minute'
            gossip_request_timeout = '10 seconds'
            get_remainder_timeout = '5 seconds'
            validate_and_store_timeout = '1 minute'
            next_round_success_threshold = 2

            [max_rounds]
            block = 5
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.next_round_success_threshold, 2);
        assert_eq!(
            config.max_rounds,
            MaxGossipRounds {
                block: 5,
                ..MaxGossipRounds::default()
            }
        );
    }
}
