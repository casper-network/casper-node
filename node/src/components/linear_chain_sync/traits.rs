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

pub(crate) trait ReactorEventT:
    From<StorageRequest>
    + From<FetcherRequest<Block>>
    + From<FetcherRequest<BlockByHeight>>
    + From<BlockValidationRequest>
    + From<ContractRuntimeRequest>
    + From<StateStoreRequest>
    + From<ControlAnnouncement>
    + From<ContractRuntimeAnnouncement>
    + Send
{
}

impl<REv> ReactorEventT for REv where
    REv: From<StorageRequest>
        + From<FetcherRequest<Block>>
        + From<FetcherRequest<BlockByHeight>>
        + From<BlockValidationRequest>
        + From<ContractRuntimeRequest>
        + From<StateStoreRequest>
        + From<ControlAnnouncement>
        + From<ContractRuntimeAnnouncement>
        + Send
{
}
