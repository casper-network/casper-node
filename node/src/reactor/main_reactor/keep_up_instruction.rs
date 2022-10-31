use std::time::Duration;

use crate::{effect::Effects, reactor::main_reactor::MainEvent};

pub(super) enum KeepUpInstruction {
    Validate(Effects<MainEvent>),
    Do(Duration, Effects<MainEvent>),
    CheckLater(String, Duration),
    CatchUp,
}
