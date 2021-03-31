use std::fmt::Debug;

use crate::{
    effect::requests::{
        BlockValidationRequest, ContractRuntimeRequest, FetcherRequest, StorageRequest,
    },
    types::{Block, BlockWithMetadata},
};

pub trait ReactorEventT<I: Debug>:
    From<StorageRequest>
    + From<FetcherRequest<I, Block>>
    + From<FetcherRequest<I, BlockWithMetadata>>
    + From<BlockValidationRequest<Block, I>>
    + From<ContractRuntimeRequest>
    + Send
{
}

impl<I, REv> ReactorEventT<I> for REv
where
    REv: From<StorageRequest>
        + From<FetcherRequest<I, Block>>
        + From<FetcherRequest<I, BlockWithMetadata>>
        + From<BlockValidationRequest<Block, I>>
        + From<ContractRuntimeRequest>
        + Send,
    I: Debug,
{
}
