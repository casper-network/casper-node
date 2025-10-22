use std::{
    fmt::{Display, Formatter},
    net::{IpAddr, SocketAddr},
};

use casper_binary_port::{BinaryResponse, Command, GetRequest};
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
        request: Command,
        peer_ip: IpAddr,
        responder: Responder<BinaryResponse>,
    },
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Event::Initialize => write!(f, "initialize"),
            Event::AcceptConnection { peer, .. } => write!(f, "accept connection from {}", peer),
            Event::HandleRequest {
                request, peer_ip, ..
            } => match request {
                Command::Get(request) => match request {
                    GetRequest::Record {
                        record_type_tag,
                        key,
                    } => {
                        write!(
                            f,
                            "get record with tag {} ({}) from {}",
                            record_type_tag,
                            key.len(),
                            peer_ip
                        )
                    }
                    GetRequest::Information { info_type_tag, key } => {
                        write!(
                            f,
                            "get info with tag {} ({}) from {}",
                            info_type_tag,
                            key.len(),
                            peer_ip
                        )
                    }
                    GetRequest::State(state_request) => {
                        write!(f, "get state ({}) from {}", state_request.as_ref(), peer_ip)
                    }
                    GetRequest::Trie { trie_key } => {
                        write!(f, "get trie ({}) from {}", trie_key, peer_ip)
                    }
                },
                Command::TryAcceptTransaction { transaction, .. } => {
                    write!(
                        f,
                        "try accept transaction ({}) from {}",
                        transaction.hash(),
                        peer_ip
                    )
                }
                Command::TrySpeculativeExec { transaction, .. } => {
                    write!(
                        f,
                        "try speculative exec ({}) from {}",
                        transaction.hash(),
                        peer_ip
                    )
                }

                Command::TrySandboxedExecution { .. } => {
                    write!(f, "try sandboxed execution from {}", peer_ip)
                }
            },
        }
    }
}
