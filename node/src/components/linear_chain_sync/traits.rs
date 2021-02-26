use crate::{
    effect::requests::{
        BlockValidationRequest, ContractRuntimeRequest, FetcherRequest, StateStoreRequest,
        StorageRequest,
    },
    types::{Block, BlockByHeight},
};
pub trait ReactorEventT<I>:
    From<StorageRequest>
    + From<FetcherRequest<I, Block>>
    + From<FetcherRequest<I, BlockByHeight>>
    + From<BlockValidationRequest<Block, I>>
    + From<ContractRuntimeRequest>
    + From<StateStoreRequest>
    + Send
{
}

impl<I, REv> ReactorEventT<I> for REv where
    REv: From<StorageRequest>
        + From<FetcherRequest<I, Block>>
        + From<FetcherRequest<I, BlockByHeight>>
        + From<BlockValidationRequest<Block, I>>
        + From<ContractRuntimeRequest>
        + From<StateStoreRequest>
        + Send
{
}
