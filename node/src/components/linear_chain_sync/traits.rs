use crate::{
    effect::requests::{
        BlockExecutorRequest, BlockValidationRequest, FetcherRequest, StorageRequest,
    },
    types::{Block, BlockByHeight, BlockHeader},
};
pub trait ReactorEventT<I>:
    From<StorageRequest>
    + From<FetcherRequest<I, Block>>
    + From<FetcherRequest<I, BlockByHeight>>
    + From<BlockValidationRequest<BlockHeader, I>>
    + From<BlockExecutorRequest>
    + Send
{
}

impl<I, REv> ReactorEventT<I> for REv where
    REv: From<StorageRequest>
        + From<FetcherRequest<I, Block>>
        + From<FetcherRequest<I, BlockByHeight>>
        + From<BlockValidationRequest<BlockHeader, I>>
        + From<BlockExecutorRequest>
        + Send
{
}
