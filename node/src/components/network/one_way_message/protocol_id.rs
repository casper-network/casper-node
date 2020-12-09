use libp2p::core::ProtocolName;
use semver::Version;

const MAX_PROTOCOL_ID_LENGTH: usize = 140;

/// The protocol ID for the `OneWayCodec`.
#[derive(Clone, Debug)]
pub struct ProtocolId {
    id: String,
}

impl ProtocolId {
    pub(super) fn new(chain_name: &str, protocol_version: &Version) -> Self {
        let id = format!(
            "/casper/{}/validator/one-way/{}",
            chain_name, protocol_version
        );

        assert!(
            id.as_bytes().len() <= MAX_PROTOCOL_ID_LENGTH,
            "Protocol IDs must not exceed {} bytes in length",
            MAX_PROTOCOL_ID_LENGTH
        );

        ProtocolId { id }
    }
}

impl ProtocolName for ProtocolId {
    fn protocol_name(&self) -> &[u8] {
        self.id.as_bytes()
    }
}
