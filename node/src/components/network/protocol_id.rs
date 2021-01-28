use libp2p::core::ProtocolName;

use crate::components::chainspec_loader::Chainspec;

/// The max length of protocol ID supported by libp2p.  See
/// https://docs.rs/libp2p/0.22.0/libp2p/core/trait.ProtocolName.html#tymethod.protocol_name
const MAX_PROTOCOL_ID_LENGTH: usize = 140;

/// A protocol ID.
#[derive(Clone, Debug)]
pub(super) struct ProtocolId {
    id: String,
}

impl ProtocolId {
    pub(super) fn new(chainspec: &Chainspec, name: &str) -> Self {
        let id = format!(
            "/casper/{}/{}/{}",
            chainspec.genesis.name, name, chainspec.genesis.protocol_version
        );

        assert!(
            id.as_bytes().len() <= MAX_PROTOCOL_ID_LENGTH,
            "Protocol IDs must not exceed {} bytes in length",
            MAX_PROTOCOL_ID_LENGTH
        );

        ProtocolId { id }
    }

    pub(super) fn id(&self) -> String {
        self.id.clone()
    }
}

impl ProtocolName for ProtocolId {
    fn protocol_name(&self) -> &[u8] {
        self.id.as_bytes()
    }
}
