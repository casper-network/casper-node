use std::fmt::Debug;

use crate::{
    effect::{
        announcements::ControlAnnouncement,
        requests::{
            BlockValidationRequest, ContractRuntimeRequest, FetcherRequest, StateStoreRequest,
            StorageRequest,
        },
    },
    types::{Block, BlockWithMetadata},
};

pub trait ReactorEventT<I: Debug + Eq>:
    From<StorageRequest>
    + From<FetcherRequest<I, Block>>
    + From<FetcherRequest<I, BlockWithMetadata>>
    + From<BlockValidationRequest<Block, I>>
    + From<ContractRuntimeRequest>
    + From<StateStoreRequest>
    + From<ControlAnnouncement>
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
        + From<StateStoreRequest>
        + From<ControlAnnouncement>
        + Send,
    I: Debug + Eq,
{
}
