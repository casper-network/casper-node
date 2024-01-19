mod functions_disabled;

use std::fmt::{self, Display, Formatter};

use derive_more::From;
use serde::Serialize;

use casper_types::{
    binary_port::{binary_request::BinaryRequest, get::GetRequest},
    Digest, KeyTag,
};

use crate::{
    components::binary_port::event::Event as BinaryPortEvent,
    effect::{
        announcements::ControlAnnouncement,
        requests::{
            AcceptTransactionRequest, BlockSynchronizerRequest, ChainspecRawBytesRequest,
            ConsensusRequest, ContractRuntimeRequest, NetworkInfoRequest, ReactorInfoRequest,
            StorageRequest, UpgradeWatcherRequest,
        },
    },
    reactor::ReactorEvent,
};

use super::BinaryPort;

/// Top-level event for the test reactors.
#[derive(Debug, From, Serialize)]
#[must_use]
enum Event {
    #[from]
    BinaryPort(#[serde(skip_serializing)] BinaryPortEvent),
    #[from]
    ControlAnnouncement(ControlAnnouncement),
    #[from]
    ContractRuntimeRequest(ContractRuntimeRequest),
    #[from]
    ReactorInfoRequest(ReactorInfoRequest),
}

impl From<ChainspecRawBytesRequest> for Event {
    fn from(_request: ChainspecRawBytesRequest) -> Self {
        unreachable!()
    }
}

impl From<UpgradeWatcherRequest> for Event {
    fn from(_request: UpgradeWatcherRequest) -> Self {
        unreachable!()
    }
}

impl From<BlockSynchronizerRequest> for Event {
    fn from(_request: BlockSynchronizerRequest) -> Self {
        unreachable!()
    }
}

impl From<ConsensusRequest> for Event {
    fn from(_request: ConsensusRequest) -> Self {
        unreachable!()
    }
}

impl From<NetworkInfoRequest> for Event {
    fn from(_request: NetworkInfoRequest) -> Self {
        unreachable!()
    }
}

impl From<AcceptTransactionRequest> for Event {
    fn from(_request: AcceptTransactionRequest) -> Self {
        unreachable!()
    }
}

impl From<StorageRequest> for Event {
    fn from(_request: StorageRequest) -> Self {
        unreachable!()
    }
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::ControlAnnouncement(ctrl_ann) => write!(formatter, "control: {}", ctrl_ann),
            Event::BinaryPort(request) => write!(formatter, "binary port request: {:?}", request),
            Event::ContractRuntimeRequest(request) => {
                write!(formatter, "contract runtime request: {:?}", request)
            }
            Event::ReactorInfoRequest(request) => {
                write!(formatter, "reactor info request: {:?}", request)
            }
        }
    }
}

impl ReactorEvent for Event {
    fn is_control(&self) -> bool {
        matches!(self, Event::ControlAnnouncement(_))
    }

    fn try_into_control(self) -> Option<ControlAnnouncement> {
        if let Self::ControlAnnouncement(ctrl_ann) = self {
            Some(ctrl_ann)
        } else {
            None
        }
    }
}

fn all_values_request() -> BinaryRequest {
    BinaryRequest::Get(GetRequest::AllValues {
        state_root_hash: Digest::hash([1u8; 32]),
        key_tag: KeyTag::Account,
    })
}

fn trie_request() -> BinaryRequest {
    BinaryRequest::Get(GetRequest::Trie {
        trie_key: Digest::hash([1u8; 32]),
    })
}
