use alloc::{string::String, vec::Vec};
#[cfg(feature = "json-schema")]
use once_cell::sync::Lazy;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    BlockHash,
};

#[cfg(test)]
use rand::Rng;

#[cfg(test)]
use crate::testing::TestRng;

#[cfg(feature = "json-schema")]
static BLOCK_SYNCHRONIZER_STATUS: Lazy<BlockSynchronizerStatus> = Lazy::new(|| {
    use crate::Digest;

    BlockSynchronizerStatus::new(
        Some(BlockSyncStatus {
            block_hash: BlockHash::new(
                Digest::from_hex(
                    "16ddf28e2b3d2e17f4cef36f8b58827eca917af225d139b0c77df3b4a67dc55e",
                )
                .unwrap(),
            ),
            block_height: Some(40),
            acquisition_state: "have strict finality(40) for: block hash 16dd..c55e".to_string(),
        }),
        Some(BlockSyncStatus {
            block_hash: BlockHash::new(
                Digest::from_hex(
                    "59907b1e32a9158169c4d89d9ce5ac9164fc31240bfcfb0969227ece06d74983",
                )
                .unwrap(),
            ),
            block_height: Some(6701),
            acquisition_state: "have block body(6701) for: block hash 5990..4983".to_string(),
        }),
    )
});

/// The status of syncing an individual block.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BlockSyncStatus {
    /// The block hash.
    block_hash: BlockHash,
    /// The height of the block, if known.
    block_height: Option<u64>,
    /// The state of acquisition of the data associated with the block.
    acquisition_state: String,
}

impl BlockSyncStatus {
    /// Constructs a new `BlockSyncStatus`.
    pub fn new(
        block_hash: BlockHash,
        block_height: Option<u64>,
        acquisition_state: String,
    ) -> Self {
        Self {
            block_hash,
            block_height,
            acquisition_state,
        }
    }

    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        Self {
            block_hash: BlockHash::random(rng),
            block_height: rng.gen::<bool>().then_some(rng.gen()),
            acquisition_state: rng.random_string(10..20),
        }
    }
}

impl ToBytes for BlockSyncStatus {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.block_hash.write_bytes(writer)?;
        self.block_height.write_bytes(writer)?;
        self.acquisition_state.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        self.block_hash.serialized_length()
            + self.block_height.serialized_length()
            + self.acquisition_state.serialized_length()
    }
}

impl FromBytes for BlockSyncStatus {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (block_hash, remainder) = BlockHash::from_bytes(bytes)?;
        let (block_height, remainder) = Option::<u64>::from_bytes(remainder)?;
        let (acquisition_state, remainder) = String::from_bytes(remainder)?;
        Ok((
            BlockSyncStatus {
                block_hash,
                block_height,
                acquisition_state,
            },
            remainder,
        ))
    }
}

/// The status of the block synchronizer.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BlockSynchronizerStatus {
    /// The status of syncing a historical block, if any.
    historical: Option<BlockSyncStatus>,
    /// The status of syncing a forward block, if any.
    forward: Option<BlockSyncStatus>,
}

impl BlockSynchronizerStatus {
    /// Constructs a new `BlockSynchronizerStatus`.
    pub fn new(historical: Option<BlockSyncStatus>, forward: Option<BlockSyncStatus>) -> Self {
        Self {
            historical,
            forward,
        }
    }

    /// Returns an example `BlockSynchronizerStatus`.
    #[cfg(feature = "json-schema")]
    pub fn example() -> &'static Self {
        &BLOCK_SYNCHRONIZER_STATUS
    }

    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        let historical = rng.gen::<bool>().then_some(BlockSyncStatus::random(rng));
        let forward = rng.gen::<bool>().then_some(BlockSyncStatus::random(rng));
        Self {
            historical,
            forward,
        }
    }
}

impl ToBytes for BlockSynchronizerStatus {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.historical.write_bytes(writer)?;
        self.forward.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        self.historical.serialized_length() + self.forward.serialized_length()
    }
}

impl FromBytes for BlockSynchronizerStatus {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (historical, remainder) = Option::<BlockSyncStatus>::from_bytes(bytes)?;
        let (forward, remainder) = Option::<BlockSyncStatus>::from_bytes(remainder)?;
        Ok((
            BlockSynchronizerStatus {
                historical,
                forward,
            },
            remainder,
        ))
    }
}
