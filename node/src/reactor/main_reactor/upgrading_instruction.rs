use std::time::Duration;

pub(super) enum UpgradingInstruction {
    CheckLater(String, Duration),
    CatchUp,
    Fatal(String),
}

impl UpgradingInstruction {
    pub(super) fn should_commit_upgrade(
        should_commit_upgrade: Result<bool, String>,
        wait: Duration,
    ) -> UpgradingInstruction {
        match should_commit_upgrade {
            Ok(true) => UpgradingInstruction::CheckLater("awaiting upgrade".to_string(), wait),
            Ok(false) => UpgradingInstruction::CatchUp,
            Err(msg) => UpgradingInstruction::Fatal(msg),
        }
    }
}
