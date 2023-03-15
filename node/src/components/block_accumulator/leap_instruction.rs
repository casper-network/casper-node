use std::fmt::{Display, Formatter};

#[derive(Debug, PartialEq)]
pub(super) enum LeapInstruction {
    // should not leap
    AtHighestKnownBlock,
    WithinAttemptExecutionThreshold(u64),
    TooCloseToUpgradeBoundary(u64),
    NoUsableBlockAcceptors,

    // should leap
    UnsetLocalTip,
    UnknownBlockHeight,
    OutsideAttemptExecutionThreshold(u64),
}

impl LeapInstruction {
    pub(super) fn from_execution_threshold(
        attempt_execution_threshold: u64,
        distance_from_highest_known_block: u64,
        is_upgrade_boundary: bool,
    ) -> Self {
        if distance_from_highest_known_block == 0 {
            return LeapInstruction::AtHighestKnownBlock;
        }
        // allow double the execution threshold back off as a safety margin
        if is_upgrade_boundary
            && distance_from_highest_known_block <= attempt_execution_threshold * 2
        {
            return LeapInstruction::TooCloseToUpgradeBoundary(distance_from_highest_known_block);
        }
        if distance_from_highest_known_block > attempt_execution_threshold {
            return LeapInstruction::OutsideAttemptExecutionThreshold(
                distance_from_highest_known_block,
            );
        }
        LeapInstruction::WithinAttemptExecutionThreshold(distance_from_highest_known_block)
    }

    pub(super) fn should_leap(&self) -> bool {
        match self {
            LeapInstruction::AtHighestKnownBlock
            | LeapInstruction::WithinAttemptExecutionThreshold(_)
            | LeapInstruction::TooCloseToUpgradeBoundary(_)
            | LeapInstruction::NoUsableBlockAcceptors => false,
            LeapInstruction::UnsetLocalTip
            | LeapInstruction::UnknownBlockHeight
            | LeapInstruction::OutsideAttemptExecutionThreshold(_) => true,
        }
    }
}

impl Display for LeapInstruction {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            LeapInstruction::AtHighestKnownBlock => {
                write!(f, "at highest known block")
            }
            LeapInstruction::TooCloseToUpgradeBoundary(diff) => {
                write!(f, "{} blocks away from protocol upgrade", diff)
            }
            LeapInstruction::WithinAttemptExecutionThreshold(diff) => {
                write!(
                    f,
                    "within attempt_execution_threshold, {} blocks behind highest known block",
                    diff
                )
            }
            LeapInstruction::OutsideAttemptExecutionThreshold(diff) => {
                write!(
                    f,
                    "outside attempt_execution_threshold, {} blocks behind highest known block",
                    diff
                )
            }
            LeapInstruction::UnsetLocalTip => {
                write!(f, "block accumulator local tip is unset")
            }
            LeapInstruction::UnknownBlockHeight => {
                write!(f, "unknown block height")
            }
            LeapInstruction::NoUsableBlockAcceptors => {
                write!(
                    f,
                    "currently have no block acceptor instances with sufficient finality"
                )
            }
        }
    }
}
