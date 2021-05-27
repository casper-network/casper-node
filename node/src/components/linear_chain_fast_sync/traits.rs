use crate::{
    effect::requests::{
        BlockExecutorRequest, BlockValidationRequest, FetcherRequest, StorageRequest,
    },
    types::{Block, BlockByHeight},
};
pub trait ReactorEventT<I>:
    From<StorageRequest>
    + From<FetcherRequest<I, Block>>
    + From<FetcherRequest<I, BlockByHeight>>
    + From<BlockValidationRequest<I>>
    + From<ContractRuntimeRequest>
    + Send
{
}

impl<I, REv> ReactorEventT<I> for REv where
    REv: From<StorageRequest>
        + From<FetcherRequest<I, Block>>
        + From<FetcherRequest<I, BlockByHeight>>
        + From<BlockValidationRequest<I>>
        + From<ContractRuntimeRequest>
        + Send
{
}
