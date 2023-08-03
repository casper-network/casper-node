use datasize::DataSize;

use crate::types::BlockHeader;
use casper_types::{TimeDiff, Timestamp};

#[derive(DataSize, Debug)]
pub struct MaxTtl(TimeDiff);

impl MaxTtl {
    /// Create instance.
    pub fn new(max_ttl: TimeDiff) -> Self {
        MaxTtl(max_ttl)
    }

    /// Get inner value.
    pub fn value(&self) -> TimeDiff {
        self.0
    }

    /// Determine if two timestamps are more than max_ttl apart.
    pub fn ttl_elapsed(&self, higher: Timestamp, lower: Timestamp) -> Result<bool, String> {
        if lower > higher {
            Err("invalid timestamp chronology".to_string())
        } else {
            Ok(lower < higher.saturating_sub(self.0))
        }
    }

    /// Determine if orphaned block header is older than ttl requires.
    pub fn synced_to_ttl(
        &self,
        latest_complete_switch_block_timestamp: Timestamp,
        highest_orphaned_block_header: &BlockHeader,
    ) -> Result<bool, String> {
        if highest_orphaned_block_header.is_genesis() {
            Ok(true)
        } else {
            self.ttl_elapsed(
                latest_complete_switch_block_timestamp,
                highest_orphaned_block_header.timestamp(),
            )
        }
    }
}

/// Wrap a TimeDiff as a MaxTtl.
impl From<TimeDiff> for MaxTtl {
    fn from(value: TimeDiff) -> Self {
        MaxTtl::new(value)
    }
}

#[cfg(test)]
mod tests {
    use casper_types::{testing::TestRng, ProtocolVersion, TimeDiff, Timestamp};

    use crate::types::{Block, MaxTtl};

    const SUB_MAX_TTL: TimeDiff = TimeDiff::from_millis(1);
    const MAX_TTL: TimeDiff = TimeDiff::from_millis(2);

    fn assert_ttl(
        higher: Timestamp,
        lower: Timestamp,
        max_ttl: TimeDiff,
        elapsed_expected: bool,
        msg: &str,
    ) {
        let max_ttl: MaxTtl = max_ttl.into();
        let elapsed = max_ttl
            .ttl_elapsed(higher, lower)
            .expect("should not error");
        assert_eq!(elapsed, elapsed_expected, "{}", msg);
    }

    #[test]
    fn should_elapse() {
        let higher = Timestamp::now();
        let lower = higher
            .saturating_sub(MAX_TTL)
            .saturating_sub(TimeDiff::from_millis(1));
        assert_ttl(
            higher,
            lower,
            MAX_TTL,
            true,
            "1 milli over ttl should have elapsed",
        );
    }

    #[test]
    fn should_not_elapse() {
        let higher = Timestamp::now();
        let lower = higher.saturating_sub(SUB_MAX_TTL);
        assert_ttl(higher, lower, MAX_TTL, false, "should not have elapsed");
    }

    #[test]
    fn should_not_elapse_with_equal_timestamps() {
        let timestamp = Timestamp::now();
        assert_ttl(
            timestamp,
            timestamp,
            MAX_TTL,
            false,
            "equal timestamps should not be elapsed",
        );
    }

    #[test]
    fn should_not_elapse_on_cusp() {
        let higher = Timestamp::now();
        let lower = higher.saturating_sub(MAX_TTL);
        assert_ttl(
            higher,
            lower,
            MAX_TTL,
            false,
            "should not have elapsed exactly on cusp of ttl",
        );
    }

    #[test]
    fn should_err() {
        let higher = Timestamp::now();
        let lower = higher.saturating_sub(SUB_MAX_TTL);
        let max_ttl: MaxTtl = MAX_TTL.into();
        assert!(
            max_ttl.ttl_elapsed(lower, higher).is_err(),
            "should have errored because timestamps are chronologically reversed (programmer error)"
        );
    }

    fn assert_sync_to_ttl(is_genesis: bool, ttl_synced_expected: bool, msg: &str) {
        let max_ttl: MaxTtl = MAX_TTL.into();
        let mut rng = TestRng::new();
        let (latest_switch_block_timestamp, highest_orphaned_block_header) = if is_genesis {
            let block = Block::random_with_specifics(
                &mut rng,
                0.into(),
                0,
                ProtocolVersion::default(),
                true,
                None,
            );
            // it does not matter what this value is; if genesis has been reached
            // while walking backwards, there are no earlier blocks to get
            // thus all sync scenarios have succeeded / are satisfied
            let timestamp = Timestamp::random(&mut rng);
            (timestamp, block.header().clone())
        } else {
            let block = Block::random_with_specifics(
                &mut rng,
                1.into(),
                1,
                ProtocolVersion::default(),
                false,
                None,
            );
            // project a sufficiently advanced future timestamp for the test.
            let mut timestamp = block.timestamp().saturating_add(max_ttl.value());
            if ttl_synced_expected {
                timestamp = timestamp.saturating_add(TimeDiff::from_millis(1))
            }
            (timestamp, block.header().clone())
        };
        let synced = max_ttl
            .synced_to_ttl(
                latest_switch_block_timestamp,
                &highest_orphaned_block_header,
            )
            .expect("should not error");
        assert_eq!(synced, ttl_synced_expected, "{}", msg);
    }

    #[test]
    fn should_handle_genesis_special_case() {
        assert_sync_to_ttl(
            true,
            true,
            "genesis should always satisfy sync to ttl requirement",
        );
    }

    #[test]
    fn should_be_synced_to_ttl() {
        assert_sync_to_ttl(false, true, "should be sync'd to ttl");
    }

    #[test]
    fn should_not_be_synced_to_ttl() {
        assert_sync_to_ttl(false, false, "should not be sync'd to ttl");
    }
}
