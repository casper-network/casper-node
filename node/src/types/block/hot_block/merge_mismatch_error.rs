use thiserror::Error;
use tracing::error;

#[derive(Error, Debug)]
pub(crate) enum MergeMismatchError {
    #[error("block mismatch when merging hot blocks")]
    Block,
    #[error("execution results mismatch when merging hot blocks")]
    ExecutionResults,
    #[error("state mismatch when merging hot blocks")]
    State,
}
