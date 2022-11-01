use std::time::Duration;

use crate::{effect::Effects, reactor::main_reactor::MainEvent, types::BlockHeader};

pub(super) enum CatchUpInstruction {
    Do(Duration, Effects<MainEvent>),
    CheckLater(String, Duration),
    Shutdown(String),
    CaughtUp,
    CommitGenesis,
    CommitUpgrade(Box<BlockHeader>),
}
