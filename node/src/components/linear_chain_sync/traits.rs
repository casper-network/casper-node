use std::fmt::Debug;

use crate::{
    effect::{
        announcements::{ContractRuntimeAnnouncement, ControlAnnouncement},
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
    + From<BlockValidationRequest<I>>
    + From<ContractRuntimeRequest>
    + From<StateStoreRequest>
    + From<ControlAnnouncement>
    + From<ContractRuntimeAnnouncement>
    + Send
{
}

impl<I, REv> ReactorEventT<I> for REv
where
    REv: From<StorageRequest>
        + From<FetcherRequest<I, Block>>
        + From<FetcherRequest<I, BlockWithMetadata>>
        + From<BlockValidationRequest<I>>
        + From<ContractRuntimeRequest>
        + From<StateStoreRequest>
        + From<ControlAnnouncement>
        + From<ContractRuntimeAnnouncement>
        + Send,
    I: Debug + Eq,
{
}
