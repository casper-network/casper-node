use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    AvailableBlockRange, BlockHash, BlockSynchronizerStatus, Digest, NextUpgrade, Peers,
    ProtocolVersion, PublicKey, TimeDiff, Timestamp,
};

#[cfg(test)]
use casper_types::testing::TestRng;
#[cfg(test)]
use rand::Rng;
use serde::Serialize;

use crate::{minimal_block_info::MinimalBlockInfo, type_wrappers::ReactorStateName};

/// Status information about the node.
#[derive(Debug, PartialEq, Serialize)]
pub struct NodeStatus {
    /// The current protocol version.
    pub protocol_version: ProtocolVersion,
    /// The node ID and network address of each connected peer.
    pub peers: Peers,
    /// The compiled node version.
    pub build_version: String,
    /// The chainspec name.
    pub chainspec_name: String,
    /// The state root hash of the lowest block in the available block range.
    pub starting_state_root_hash: Digest,
    /// The minimal info of the last block from the linear chain.
    pub last_added_block_info: Option<MinimalBlockInfo>,
    /// Our public signing key.
    pub our_public_signing_key: Option<PublicKey>,
    /// The next round length if this node is a validator.
    pub round_length: Option<TimeDiff>,
    /// Information about the next scheduled upgrade.
    pub next_upgrade: Option<NextUpgrade>,
    /// Time that passed since the node has started.
    pub uptime: TimeDiff,
    /// The current state of node reactor.
    pub reactor_state: ReactorStateName,
    /// Timestamp of the last recorded progress in the reactor.
    pub last_progress: Timestamp,
    /// The available block range in storage.
    pub available_block_range: AvailableBlockRange,
    /// The status of the block synchronizer builders.
    pub block_sync: BlockSynchronizerStatus,
    /// The hash of the latest switch block.
    pub latest_switch_block_hash: Option<BlockHash>,
}

impl NodeStatus {
    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        Self {
            protocol_version: ProtocolVersion::from_parts(rng.gen(), rng.gen(), rng.gen()),
            peers: Peers::random(rng),
            build_version: rng.random_string(5..10),
            chainspec_name: rng.random_string(5..10),
            starting_state_root_hash: Digest::random(rng),
            last_added_block_info: rng.gen::<bool>().then_some(MinimalBlockInfo::random(rng)),
            our_public_signing_key: rng.gen::<bool>().then_some(PublicKey::random(rng)),
            round_length: rng
                .gen::<bool>()
                .then_some(TimeDiff::from_millis(rng.gen())),
            next_upgrade: rng.gen::<bool>().then_some(NextUpgrade::random(rng)),
            uptime: TimeDiff::from_millis(rng.gen()),
            reactor_state: ReactorStateName::new(rng.random_string(5..10)),
            last_progress: Timestamp::random(rng),
            available_block_range: AvailableBlockRange::random(rng),
            block_sync: BlockSynchronizerStatus::random(rng),
            latest_switch_block_hash: rng.gen::<bool>().then_some(BlockHash::random(rng)),
        }
    }
}

impl FromBytes for NodeStatus {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (protocol_version, remainder) = ProtocolVersion::from_bytes(bytes)?;
        let (peers, remainder) = Peers::from_bytes(remainder)?;
        let (build_version, remainder) = String::from_bytes(remainder)?;
        let (chainspec_name, remainder) = String::from_bytes(remainder)?;
        let (starting_state_root_hash, remainder) = Digest::from_bytes(remainder)?;
        let (last_added_block_info, remainder) = Option::<MinimalBlockInfo>::from_bytes(remainder)?;
        let (our_public_signing_key, remainder) = Option::<PublicKey>::from_bytes(remainder)?;
        let (round_length, remainder) = Option::<TimeDiff>::from_bytes(remainder)?;
        let (next_upgrade, remainder) = Option::<NextUpgrade>::from_bytes(remainder)?;
        let (uptime, remainder) = TimeDiff::from_bytes(remainder)?;
        let (reactor_state, remainder) = ReactorStateName::from_bytes(remainder)?;
        let (last_progress, remainder) = Timestamp::from_bytes(remainder)?;
        let (available_block_range, remainder) = AvailableBlockRange::from_bytes(remainder)?;
        let (block_sync, remainder) = BlockSynchronizerStatus::from_bytes(remainder)?;
        let (latest_switch_block_hash, remainder) = Option::<BlockHash>::from_bytes(remainder)?;
        Ok((
            NodeStatus {
                protocol_version,
                peers,
                build_version,
                chainspec_name,
                starting_state_root_hash,
                last_added_block_info,
                our_public_signing_key,
                round_length,
                next_upgrade,
                uptime,
                reactor_state,
                last_progress,
                available_block_range,
                block_sync,
                latest_switch_block_hash,
            },
            remainder,
        ))
    }
}

impl ToBytes for NodeStatus {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        let NodeStatus {
            protocol_version,
            peers,
            build_version,
            chainspec_name,
            starting_state_root_hash,
            last_added_block_info,
            our_public_signing_key,
            round_length,
            next_upgrade,
            uptime,
            reactor_state,
            last_progress,
            available_block_range,
            block_sync,
            latest_switch_block_hash,
        } = self;
        protocol_version.write_bytes(writer)?;
        peers.write_bytes(writer)?;
        build_version.write_bytes(writer)?;
        chainspec_name.write_bytes(writer)?;
        starting_state_root_hash.write_bytes(writer)?;
        last_added_block_info.write_bytes(writer)?;
        our_public_signing_key.write_bytes(writer)?;
        round_length.write_bytes(writer)?;
        next_upgrade.write_bytes(writer)?;
        uptime.write_bytes(writer)?;
        reactor_state.write_bytes(writer)?;
        last_progress.write_bytes(writer)?;
        available_block_range.write_bytes(writer)?;
        block_sync.write_bytes(writer)?;
        latest_switch_block_hash.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        self.protocol_version.serialized_length()
            + self.peers.serialized_length()
            + self.build_version.serialized_length()
            + self.chainspec_name.serialized_length()
            + self.starting_state_root_hash.serialized_length()
            + self.last_added_block_info.serialized_length()
            + self.our_public_signing_key.serialized_length()
            + self.round_length.serialized_length()
            + self.next_upgrade.serialized_length()
            + self.uptime.serialized_length()
            + self.reactor_state.serialized_length()
            + self.last_progress.serialized_length()
            + self.available_block_range.serialized_length()
            + self.block_sync.serialized_length()
            + self.latest_switch_block_hash.serialized_length()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use casper_types::testing::TestRng;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let val = NodeStatus::random(rng);
        bytesrepr::test_serialization_roundtrip(&val);
    }
}
