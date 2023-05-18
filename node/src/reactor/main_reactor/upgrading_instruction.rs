use std::time::Duration;

use casper_types::{TimeDiff, Timestamp};

const UPGRADE_TIMEOUT_SECONDS: u32 = 30;

pub(super) enum UpgradingInstruction {
    CheckLater(String, Duration),
    CatchUp,
}

impl UpgradingInstruction {
    pub(super) fn should_commit_upgrade(
        should_commit_upgrade: bool,
        wait: Duration,
        last_progress: Timestamp,
    ) -> UpgradingInstruction {
        if should_commit_upgrade {
            if last_progress.elapsed() > TimeDiff::from_seconds(UPGRADE_TIMEOUT_SECONDS) {
                UpgradingInstruction::CatchUp
            } else {
                UpgradingInstruction::CheckLater("awaiting upgrade".to_string(), wait)
            }
        } else {
            UpgradingInstruction::CatchUp
        }
    }
}
