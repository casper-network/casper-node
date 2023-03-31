use std::time::Duration;

pub(super) enum UpgradingInstruction {
    CheckLater(String, Duration),
    CatchUp,
}

impl UpgradingInstruction {
    pub(super) fn should_commit_upgrade(
        should_commit_upgrade: bool,
        wait: Duration,
    ) -> UpgradingInstruction {
        if should_commit_upgrade {
            UpgradingInstruction::CheckLater("awaiting upgrade".to_string(), wait)
        } else {
            UpgradingInstruction::CatchUp
        }
    }
}
