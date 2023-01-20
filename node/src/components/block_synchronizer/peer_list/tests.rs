use super::*;

impl PeerList {
    pub(crate) fn is_peer_unreliable(&self, peer_id: &NodeId) -> bool {
        *self.peer_list.get(peer_id).unwrap() == PeerQuality::Unreliable
    }
}
