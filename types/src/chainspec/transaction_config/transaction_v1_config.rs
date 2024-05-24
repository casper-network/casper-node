#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
use serde::{Deserialize, Serialize};

#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    transaction::TransactionCategory,
    INSTALL_UPGRADE_LANE_ID,
};

/// Default gas limit of install / upgrade contracts
pub const DEFAULT_INSTALL_UPGRADE_GAS_LIMIT: u64 = 3_500_000_000_000;

/// Default gas limit of standard transactions
pub const DEFAULT_LARGE_TRANSACTION_GAS_LIMIT: u64 = 500_000_000_000;

const DEFAULT_NATIVE_MINT_LANE: [u64; 5] = [0, 1_048_576, 1024, 2_500_000_000, 650];
const DEFAULT_NATIVE_AUCTION_LANE: [u64; 5] = [1, 1_048_576, 1024, 2_500_000_000, 145];

const KIND: usize = 0;
const MAX_TRANSACTION_LENGTH: usize = 1;
const MAX_TRANSACTION_ARGS_LENGTH: usize = 2;
const MAX_TRANSACTION_GAS_LIMIT: usize = 3;
const MAX_TRANSACTION_COUNT: usize = 4;

/// Configuration values associated with V1 Transactions.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct TransactionV1Config {
    /// Lane configuration of the native mint interaction.
    pub native_mint_lane: Vec<u64>,
    /// Lane configuration for the native auction interaction.
    pub native_auction_lane: Vec<u64>,
    /// Lane configurations for the Wasm based lanes.
    pub wasm_lanes: Vec<Vec<u64>>,
}

impl TransactionV1Config {
    #[cfg(any(feature = "testing", test))]
    /// Generates a random instance using a `TestRng`.
    pub fn random(rng: &mut TestRng) -> Self {
        let native_mint_lane = DEFAULT_NATIVE_MINT_LANE.to_vec();
        let native_auction_lane = DEFAULT_NATIVE_AUCTION_LANE.to_vec();
        let mut wasm_lanes = vec![];
        for kind in 2..7 {
            let lane = vec![
                kind as u64,
                rng.gen_range(0..=1_048_576),
                rng.gen_range(0..=1024),
                rng.gen_range(0..=2_500_000_000),
            ];
            wasm_lanes.push(lane)
        }

        TransactionV1Config {
            native_mint_lane,
            native_auction_lane,
            wasm_lanes,
        }
    }

    /// Returns true if the lane identifier is for either the mint or auction.
    pub fn is_native_lane(&self, lane: u8) -> bool {
        lane as u64 == DEFAULT_NATIVE_MINT_LANE[0] || lane as u64 == DEFAULT_NATIVE_AUCTION_LANE[0]
    }

    /// Returns the max serialized length of a transaction for the given category.
    pub fn get_max_serialized_length(&self, category: u8) -> u64 {
        if !self.is_supported(category) {
            return 0;
        }
        match category {
            0 => self.native_mint_lane[MAX_TRANSACTION_LENGTH],
            1 => self.native_auction_lane[MAX_TRANSACTION_LENGTH],
            _ => {
                match self
                    .wasm_lanes
                    .iter()
                    .find(|lane| lane.first() == Some(&(category as u64)))
                {
                    Some(wasm_lane) => wasm_lane[MAX_TRANSACTION_LENGTH],
                    None => 0,
                }
            }
        }
    }

    /// Returns the max serialized args length of a transaction for the given category.
    pub fn get_max_args_length(&self, category: u8) -> u64 {
        if !self.is_supported(category) {
            return 0;
        }
        match category {
            0 => self.native_mint_lane[MAX_TRANSACTION_ARGS_LENGTH],
            1 => self.native_auction_lane[MAX_TRANSACTION_ARGS_LENGTH],
            _ => {
                match self
                    .wasm_lanes
                    .iter()
                    .find(|lane| lane.first() == Some(&(category as u64)))
                {
                    Some(wasm_lane) => wasm_lane[MAX_TRANSACTION_ARGS_LENGTH],
                    None => 0,
                }
            }
        }
    }

    /// Returns the max gas limit of a transaction for the given category.
    pub fn get_max_gas_limit(&self, category: u8) -> u64 {
        if !self.is_supported(category) {
            return 0;
        }
        match category {
            0 => self.native_mint_lane[MAX_TRANSACTION_GAS_LIMIT],
            1 => self.native_auction_lane[MAX_TRANSACTION_GAS_LIMIT],
            _ => {
                match self
                    .wasm_lanes
                    .iter()
                    .find(|lane| lane.first() == Some(&(category as u64)))
                {
                    Some(wasm_lane) => wasm_lane[MAX_TRANSACTION_GAS_LIMIT],
                    None => 0,
                }
            }
        }
    }

    /// Returns the max gas limit of a transaction for the given category.
    pub fn get_max_transaction_count(&self, category: u8) -> u64 {
        if !self.is_supported(category) {
            return 0;
        }
        match category {
            0 => self.native_mint_lane[MAX_TRANSACTION_COUNT],
            1 => self.native_auction_lane[MAX_TRANSACTION_COUNT],
            _ => {
                match self
                    .wasm_lanes
                    .iter()
                    .find(|lane| lane.first() == Some(&(category as u64)))
                {
                    Some(wasm_lane) => wasm_lane[MAX_TRANSACTION_COUNT],
                    None => 0,
                }
            }
        }
    }

    /// Returns true if the given category is supported.
    pub fn is_supported(&self, category: u8) -> bool {
        if !self.is_native_lane(category) {
            return self
                .wasm_lanes
                .iter()
                .any(|lane| lane.first() == Some(&(category as u64)));
        }

        true
    }

    /// Returns the max total count for all transactions across all lanes allowed in a block.
    pub fn get_max_block_count(&self) -> u64 {
        self.native_mint_lane[MAX_TRANSACTION_COUNT]
            + self.native_auction_lane[MAX_TRANSACTION_COUNT]
            + self
            .wasm_lanes
            .iter()
            .map(|lane| lane[MAX_TRANSACTION_COUNT])
            .sum::<u64>()
    }

    /// Returns the maximum number of Wasm based transactions across wasm lanes.
    pub fn get_max_wasm_transaction_count(&self) -> u64 {
        let mut ret = 0;
        for lane in self.wasm_lanes.iter() {
            ret += lane[MAX_TRANSACTION_COUNT];
        }
        ret
    }

    /// Returns the list of currently supported lane identifiers.
    pub fn get_supported_categories(&self) -> Vec<u8> {
        let mut ret = vec![0, 1];
        for lane in self.wasm_lanes.iter() {
            let lane_id = lane[KIND] as u8;
            ret.push(lane_id);
        }
        ret
    }

    /// Returns the transaction v1 configuration with the specified lane limits.
    #[cfg(any(feature = "testing", test))]
    pub fn with_count_limits(
        mut self,
        mint_count: Option<u64>,
        auction: Option<u64>,
        install: Option<u64>,
        large_limit: Option<u64>,
    ) -> Self {
        if let Some(mint_count) = mint_count {
            self.native_mint_lane[MAX_TRANSACTION_COUNT] = mint_count;
        }
        if let Some(auction_count) = auction {
            self.native_auction_lane[MAX_TRANSACTION_COUNT] = auction_count;
        }
        if let Some(install_upgrade_count) = install {
            let (index, lane) = self
                .wasm_lanes
                .iter()
                .enumerate()
                .find(|(_, lane)| lane.first() == Some(&(INSTALL_UPGRADE_LANE_ID as u64)))
                .expect("must get install upgrade lane");
            let mut updated_lane = lane.clone();
            self.wasm_lanes.remove(index);
            updated_lane[MAX_TRANSACTION_COUNT] = install_upgrade_count;
            self.wasm_lanes.push(updated_lane);
        }
        if let Some(large_limit) = large_limit {
            let (index, lane) = self
                .wasm_lanes
                .iter()
                .enumerate()
                .find(|(_, lane)| lane.first() == Some(&3))
                .expect("must get install upgrade lane");
            let mut updated_lane = lane.clone();
            self.wasm_lanes.remove(index);
            updated_lane[MAX_TRANSACTION_COUNT] = large_limit;
            self.wasm_lanes.push(updated_lane);
        }
        self
    }
}

#[cfg(any(feature = "std", test))]
impl Default for TransactionV1Config {
    fn default() -> Self {
        let large_lane = vec![
            TransactionCategory::Large as u64,
            1_048_576,
            1024,
            DEFAULT_LARGE_TRANSACTION_GAS_LIMIT,
            10,
        ];

        let install_upgrade_lane = vec![
            TransactionCategory::InstallUpgrade as u64,
            1_048_576,
            2048,
            DEFAULT_INSTALL_UPGRADE_GAS_LIMIT,
            2,
        ];

        let native_mint_lane = DEFAULT_NATIVE_MINT_LANE.to_vec();
        let native_auction_lane = DEFAULT_NATIVE_AUCTION_LANE.to_vec();
        let wasm_lanes = vec![large_lane, install_upgrade_lane];

        TransactionV1Config {
            native_mint_lane,
            native_auction_lane,
            wasm_lanes,
        }
    }
}

impl ToBytes for TransactionV1Config {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.native_mint_lane.write_bytes(writer)?;
        self.native_auction_lane.write_bytes(writer)?;
        self.wasm_lanes.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.native_mint_lane.serialized_length()
            + self.native_auction_lane.serialized_length()
            + self.wasm_lanes.serialized_length()
    }
}

impl FromBytes for TransactionV1Config {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (native_mint_lane, remainder) = FromBytes::from_bytes(bytes)?;
        let (native_auction_lane, remainder) = FromBytes::from_bytes(remainder)?;
        let (wasm_lanes, remainder) = FromBytes::from_bytes(remainder)?;
        let config = TransactionV1Config {
            native_mint_lane,
            native_auction_lane,
            wasm_lanes,
        };
        Ok((config, remainder))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let mut rng = TestRng::new();
        let config = TransactionV1Config::random(&mut rng);
        bytesrepr::test_serialization_roundtrip(&config);
    }

    #[test]
    fn should_correctly_track_supported() {
        let config = TransactionV1Config::default();
        assert!(config.is_supported(0));
        assert!(config.is_supported(1));
        assert!(config.is_supported(3));
        assert!(!config.is_supported(10));
    }
}
