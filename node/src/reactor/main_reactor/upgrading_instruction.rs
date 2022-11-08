use std::time::Duration;

pub(super) enum UpgradingInstruction {
    CheckLater(String, Duration),
    CatchUp,
    Shutdown(String),
}
