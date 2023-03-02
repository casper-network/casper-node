use super::*;

impl BlockBuilder {
    pub(crate) fn peer_list(&self) -> &PeerList {
        &self.peer_list
    }
}
