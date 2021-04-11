pub mod outgoing;

// use std::net::SocketAddr;
// use std::{
//     collections::{HashMap, HashSet},
//     time::Instant,
// };

type NodeId = u8;

// struct Outgoing {
//     addr: SocketAddr,
//     state: OutgoingState,
// }

// enum OutgoingState {
//     Known,
//     Failed,
//     Connected,
//     Blocked,
// }

// struct OutgoingManager {
//     outgoing: HashMap<SocketAddr, Outgoing>,
//     routes: HashMap<NodeId, SocketAddr>,
// }

// struct Incoming {
//     addr: SocketAddr,
// }

// struct BlacklistEntry {
//     blacklisted_since: Instant,
//     known_peer_addresses: SocketAddr,
// }

// struct PeerManager {
//     routes: HashMap<NodeId, SocketAddr>,
//     outgoing: HashMap<SocketAddr, Outgoing>,
//     blocked_nodes: HashSet<NodeId>,
// }

// struct RemotePeer {}

// enum RemotePeerState {
//     OutgoingOnly,
//     Established,
//     Blacklisted,
// }

// #[cfg(test)]
// mod tests {
//     #[test]
//     fn it_works() {
//         assert_eq!(2 + 2, 4);
//     }
// }
