use std::{
    fmt::{self, Display, Formatter},
    mem,
};

use derive_more::From;
use serde::Serialize;
use static_assertions::const_assert;

use crate::effect::{
    incoming::NetRequestIncoming,
    requests::{MakeBlockExecutableRequest, MarkBlockCompletedRequest, StorageRequest},
};

const _STORAGE_EVENT_SIZE: usize = mem::size_of::<Event>();
const_assert!(_STORAGE_EVENT_SIZE <= 32);

/// A storage component event.
#[derive(Debug, From, Serialize)]
#[repr(u8)]
pub(crate) enum Event {
    /// Storage request.
    #[from]
    StorageRequest(Box<StorageRequest>),
    /// Incoming net request.
    NetRequestIncoming(Box<NetRequestIncoming>),
    /// Mark block completed request.
    #[from]
    MarkBlockCompletedRequest(MarkBlockCompletedRequest),
    /// Make block executable request.
    #[from]
    MakeBlockExecutableRequest(Box<MakeBlockExecutableRequest>),
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::StorageRequest(req) => req.fmt(f),
            Event::NetRequestIncoming(incoming) => incoming.fmt(f),
            Event::MarkBlockCompletedRequest(req) => req.fmt(f),
            Event::MakeBlockExecutableRequest(req) => req.fmt(f),
        }
    }
}

impl From<NetRequestIncoming> for Event {
    #[inline]
    fn from(incoming: NetRequestIncoming) -> Self {
        Event::NetRequestIncoming(Box::new(incoming))
    }
}

impl From<StorageRequest> for Event {
    #[inline]
    fn from(request: StorageRequest) -> Self {
        Event::StorageRequest(Box::new(request))
    }
}

impl From<MakeBlockExecutableRequest> for Event {
    #[inline]
    fn from(request: MakeBlockExecutableRequest) -> Self {
        Event::MakeBlockExecutableRequest(Box::new(request))
    }
}
