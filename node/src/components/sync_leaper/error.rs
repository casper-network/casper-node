use crate::types::BlockHash;

#[derive(Debug, Clone)]
pub(crate) enum PullSyncLeapError {
    TrustedHashTooOld(BlockHash),
    CouldntFetch(BlockHash),
    OtherRequestInProgress(BlockHash),
}

#[derive(Debug, Clone)]
pub(crate) enum ConstructSyncLeapError {
    TrustedHashTooOld(BlockHash),
    TrustedHashUnknown(BlockHash),
    CouldntProve,
    StorageError,
}
