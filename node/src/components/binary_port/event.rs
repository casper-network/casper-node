use std::{
    fmt::{Display, Formatter},
    net::SocketAddr,
};

use casper_binary_port::{BinaryRequest, BinaryResponse, GetRequest, GlobalStateRequest};
use tokio::net::TcpStream;

use crate::effect::Responder;

#[derive(Debug)]
pub(crate) enum Event {
    Initialize,
    AcceptConnection {
        stream: TcpStream,
        peer: SocketAddr,
        responder: Responder<()>,
    },
    HandleRequest {
        request: BinaryRequest,
        responder: Responder<BinaryResponse>,
    },
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Event::Initialize => write!(f, "initialize"),
            Event::AcceptConnection { peer, .. } => write!(f, "accept connection from {}", peer),
            Event::HandleRequest { request, .. } => match request {
                BinaryRequest::Get(request) => match request {
                    GetRequest::Record {
                        record_type_tag,
                        key,
                    } => {
                        write!(f, "get record with tag {} ({})", record_type_tag, key.len())
                    }
                    GetRequest::Information { info_type_tag, key } => {
                        write!(f, "get info with tag {} ({})", info_type_tag, key.len())
                    }
                    GetRequest::State(state_request) => match state_request.as_ref() {
                        GlobalStateRequest::Item { base_key, .. } => {
                            write!(f, "get item from global state ({})", base_key)
                        }
                        GlobalStateRequest::AllItems { key_tag, .. } => {
                            write!(f, "get all items ({})", key_tag)
                        }
                        GlobalStateRequest::Trie { trie_key } => {
                            write!(f, "get trie ({})", trie_key)
                        }
                        GlobalStateRequest::DictionaryItem { .. } => {
                            write!(f, "get dictionary item")
                        }
                        GlobalStateRequest::Balance { .. } => {
                            write!(f, "get balance by state root",)
                        }
                        GlobalStateRequest::ItemsByPrefix { .. } => {
                            write!(f, "get items by prefix")
                        }
                    },
                },
                BinaryRequest::TryAcceptTransaction { transaction, .. } => {
                    write!(f, "try accept transaction ({})", transaction.hash())
                }
                BinaryRequest::TrySpeculativeExec { transaction, .. } => {
                    write!(f, "try speculative exec ({})", transaction.hash())
                }
            },
        }
    }
}
