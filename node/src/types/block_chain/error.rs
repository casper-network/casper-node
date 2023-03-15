/// BlockChain errors.
#[derive(Debug, PartialEq)]
pub enum Error {
    AttemptToProposeAboveTail,
    AttemptToFinalizeAboveTail,
}
