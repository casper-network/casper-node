use std::{
    fmt,
    fmt::{Display, Formatter},
};

use derive_more::From;
use serde::Serialize;

use crate::effect::{
    incoming::{TrieDemand, TrieRequestIncoming},
    requests::ContractRuntimeRequest,
};

#[derive(Debug, From, Serialize)]
pub(crate) enum Event {
    #[from]
    ContractRuntimeRequest(ContractRuntimeRequest),

    #[from]
    TrieRequestIncoming(TrieRequestIncoming),

    #[from]
    TrieDemand(TrieDemand),
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::ContractRuntimeRequest(req) => {
                write!(f, "contract runtime request: {}", req)
            }
            Event::TrieRequestIncoming(req) => write!(f, "trie request incoming: {}", req),
            Event::TrieDemand(demand) => write!(f, "trie demand: {}", demand),
        }
    }
}
