use std::{
    fmt::{Display, Formatter},
    net::SocketAddr,
};

use casper_types::binary_port::{BinaryRequest, BinaryResponse, GetRequest};
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
                    GetRequest::Db { db_tag, key } => {
                        write!(f, "get from db with tag {} ({})", db_tag, key.len())
                    }
                    GetRequest::NonPersistedData(_) => write!(f, "get non-persisted data"),
                    GetRequest::State { base_key, .. } => {
                        write!(f, "get from global state ({})", base_key)
                    }
                    GetRequest::AllValues { key_tag, .. } => {
                        write!(f, "get all values ({})", key_tag)
                    }
                    GetRequest::Trie { trie_key } => write!(f, "get trie ({})", trie_key),
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
