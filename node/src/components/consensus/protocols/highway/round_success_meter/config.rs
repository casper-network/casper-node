/// The number of most recent rounds we will be keeping track of.
pub(crate) const NUM_ROUNDS_TO_CONSIDER: usize = 40;
/// The number of successful rounds that triggers us to slow down: With this many or fewer
/// successes per `NUM_ROUNDS_TO_CONSIDER`, we increase our round exponent.
pub(crate) const NUM_ROUNDS_SLOWDOWN: usize = 10;
/// The number of successful rounds that triggers us to speed up: With this many or more successes
/// per `NUM_ROUNDS_TO_CONSIDER`, we decrease our round exponent.
pub(crate) const NUM_ROUNDS_SPEEDUP: usize = 32;
/// We will try to accelerate (decrease our round exponent) every `ACCELERATION_PARAMETER` rounds if
/// we have few enough failures.
pub(crate) const ACCELERATION_PARAMETER: u64 = 40;
/// The FTT, as a percentage (i.e. `THRESHOLD = 1` means 1% of the validators' total weight), which
/// we will use for looking for a summit in order to determine a proposal's finality.
/// The required quorum in a summit we will look for to check if a round was successful is
/// determined by this FTT.
pub(crate) const THRESHOLD: u64 = 1;

/// The maximum number of failures allowed among NUM_ROUNDS_TO_CONSIDER latest rounds, with which we
/// won't increase our round length. Exceeding this threshold will mean that we should slow down.
pub(crate) const MAX_FAILED_ROUNDS: usize = NUM_ROUNDS_TO_CONSIDER - NUM_ROUNDS_SLOWDOWN - 1;
/// The maximum number of failures with which we will attempt to accelerate (decrease the round
/// exponent).
pub(crate) const MAX_FAILURES_FOR_ACCELERATION: usize = NUM_ROUNDS_TO_CONSIDER - NUM_ROUNDS_SPEEDUP;
