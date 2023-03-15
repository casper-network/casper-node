use std::time::Duration;

use crate::{effect::Effects, reactor::main_reactor::MainEvent};

pub(super) enum GenesisInstruction {
    Validator(Duration, Effects<MainEvent>),
    NonValidator(Duration, Effects<MainEvent>),
    Fatal(String),
}
