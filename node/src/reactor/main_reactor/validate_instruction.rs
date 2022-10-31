use std::time::Duration;

use crate::effect::Effects;
use crate::reactor::main_reactor::MainEvent;

pub(super) enum ValidateInstruction {
    Do(Duration, Effects<MainEvent>),
    CheckLater(String, Duration),
    NonSwitchBlock,
    KeepUp,
}
