use std::str::FromStr;

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::TimeDiff;

const DEFAULT_TIMESTAMP_LEEWAY: &str = "2sec";
const UPGRADE_TTL_LEEWAY_FOR_UPGRADE_POINT_CHECK: &str = "2hours";

/// Configuration options for accepting transactions.
#[derive(Copy, Clone, Serialize, Deserialize, Debug, DataSize)]
pub struct Config {
    /// The leeway allowed when considering whether a transaction is future-dated or not.
    ///
    /// To accommodate minor clock drift, transactions whose timestamps are within
    /// `timestamp_leeway` in the future are still acceptable.
    ///
    /// The maximum value to which `timestamp_leeway` can be set is defined by the chainspec
    /// setting `transactions.max_timestamp_leeway`.
    pub timestamp_leeway: TimeDiff,
    /// The gas price tolerance checks are disabled if the transaction TTL extends beyond the
    /// planned upgrade activation point. Since it's not possible to do the calculations precisely
    /// (due to the fact that era duration may vary and that we can't determine how much time in
    /// the current era has passed) we allow short leeway when comparing timestamps to minimize the
    /// chances of the transaction being rejected erroneously.
    ///
    /// The maximum value to which `ttl_leeway_for_upgrade_point_check` can be set is defined by
    /// the chainspec setting `transactions.max_ttl_leeway_for_upgrade_point_check`.
    pub ttl_leeway_for_upgrade_point_check: TimeDiff,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            timestamp_leeway: TimeDiff::from_str(DEFAULT_TIMESTAMP_LEEWAY).unwrap(),
            ttl_leeway_for_upgrade_point_check: TimeDiff::from_str(
                UPGRADE_TTL_LEEWAY_FOR_UPGRADE_POINT_CHECK,
            )
            .unwrap(),
        }
    }
}
