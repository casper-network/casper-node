use std::time::Duration;

use casper_types::{TimeDiff, Timestamp};

pub(super) enum UpgradingInstruction {
    CheckLater(String, Duration),
    CatchUp,
}

impl UpgradingInstruction {
    pub(super) fn should_commit_upgrade(
        should_commit_upgrade: bool,
        wait: Duration,
        last_progress: Timestamp,
        upgrade_timeout: TimeDiff,
    ) -> UpgradingInstruction {
        if should_commit_upgrade {
            if last_progress.elapsed() > upgrade_timeout {
                UpgradingInstruction::CatchUp
            } else {
                UpgradingInstruction::CheckLater("awaiting upgrade".to_string(), wait)
            }
        } else {
            UpgradingInstruction::CatchUp
        }
    }
}
