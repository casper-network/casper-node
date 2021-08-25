use crate::{
    components::contract_runtime::ContractRuntimeAnnouncement,
    effect::{
        announcements::ControlAnnouncement,
        requests::{
            BlockValidationRequest, ContractRuntimeRequest, FetcherRequest, StateStoreRequest,
            StorageRequest,
        },
    },
    types::{Block, BlockByHeight},
};

pub(crate) trait ReactorEventT<I>:
    From<StorageRequest>
    + From<FetcherRequest<I, Block>>
    + From<FetcherRequest<I, BlockByHeight>>
    + From<BlockValidationRequest<I>>
    + From<ContractRuntimeRequest>
    + From<StateStoreRequest>
    + From<ControlAnnouncement>
    + From<ContractRuntimeAnnouncement>
    + Send
{
}

impl<I, REv> ReactorEventT<I> for REv where
    REv: From<StorageRequest>
        + From<FetcherRequest<I, Block>>
        + From<FetcherRequest<I, BlockByHeight>>
        + From<BlockValidationRequest<I>>
        + From<ContractRuntimeRequest>
        + From<StateStoreRequest>
        + From<ControlAnnouncement>
        + From<ContractRuntimeAnnouncement>
        + Send
{
}
