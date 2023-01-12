use thiserror::Error;
use tracing::error;

#[derive(Error, Debug)]
pub(crate) enum MergeMismatchError {
    #[error("block mismatch when merging meta blocks")]
    Block,
    #[error("execution results mismatch when merging meta blocks")]
    ExecutionResults,
    #[error("state mismatch when merging meta blocks")]
    State,
}
