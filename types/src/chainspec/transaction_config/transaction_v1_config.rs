use core::cmp;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "once_cell", test))]
use once_cell::sync::OnceCell;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
use serde::{
    de::{Error, Unexpected},
    ser::SerializeSeq,
    Deserialize, Deserializer, Serialize, Serializer,
};

#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    AUCTION_LANE_ID, INSTALL_UPGRADE_LANE_ID, MINT_LANE_ID,
};

/// Default gas limit of standard transactions
pub const DEFAULT_LARGE_TRANSACTION_GAS_LIMIT: u64 = 6_000_000_000_000;

const DEFAULT_NATIVE_MINT_LANE: [u64; 5] = [0, 1_048_576, 1024, 2_500_000_000, 650];
const DEFAULT_NATIVE_AUCTION_LANE: [u64; 5] = [1, 1_048_576, 1024, 5_000_000_000_000, 145];
const DEFAULT_INSTALL_UPGRADE_LANE: [u64; 5] = [2, 1_048_576, 2048, 3_500_000_000_000, 2];

const TRANSACTION_ID_INDEX: usize = 0;
const TRANSACTION_LENGTH_INDEX: usize = 1;
const TRANSACTION_ARGS_LENGTH_INDEX: usize = 2;
const TRANSACTION_GAS_LIMIT_INDEX: usize = 3;
const TRANSACTION_COUNT_INDEX: usize = 4;

/// Structured limits imposed on a transaction lane
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct TransactionLaneDefinition {
    /// The lane identifier
    id: u8,
    /// The maximum length of a transaction in bytes
    max_transaction_length: u64,
    /// The max args length size in bytes
    max_transaction_args_length: u64,
    /// The maximum gas limit
    max_transaction_gas_limit: u64,
    /// The maximum number of transactions
    max_transaction_count: u64,
}

impl TryFrom<Vec<u64>> for TransactionLaneDefinition {
    type Error = TransactionConfigError;

    fn try_from(v: Vec<u64>) -> Result<Self, Self::Error> {
        if v.len() != 5 {
            return Err(TransactionConfigError::InvalidArgsProvided);
        }
        Ok(TransactionLaneDefinition {
            id: v[TRANSACTION_ID_INDEX] as u8,
            max_transaction_length: v[TRANSACTION_LENGTH_INDEX],
            max_transaction_args_length: v[TRANSACTION_ARGS_LENGTH_INDEX],
            max_transaction_gas_limit: v[TRANSACTION_GAS_LIMIT_INDEX],
            max_transaction_count: v[TRANSACTION_COUNT_INDEX],
        })
    }
}

impl TransactionLaneDefinition {
    /// Creates a new instance of TransactionLimitsDefinition
    pub fn new(
        id: u8,
        max_transaction_length: u64,
        max_transaction_args_length: u64,
        max_transaction_gas_limit: u64,
        max_transaction_count: u64,
    ) -> Self {
        Self {
            id,
            max_transaction_length,
            max_transaction_args_length,
            max_transaction_gas_limit,
            max_transaction_count,
        }
    }

    fn as_vec(&self) -> Vec<u64> {
        vec![
            self.id as u64,
            self.max_transaction_length,
            self.max_transaction_args_length,
            self.max_transaction_gas_limit,
            self.max_transaction_count,
        ]
    }

    /// Returns max_transaction_length
    pub fn max_transaction_length(&self) -> u64 {
        self.max_transaction_length
    }

    /// Returns max_transaction_args_length
    pub fn max_transaction_args_length(&self) -> u64 {
        self.max_transaction_args_length
    }

    /// Returns max_transaction_gas_limit
    pub fn max_transaction_gas_limit(&self) -> u64 {
        self.max_transaction_gas_limit
    }

    /// Returns max_transaction_count
    pub fn max_transaction_count(&self) -> u64 {
        self.max_transaction_count
    }

    /// Returns id
    pub fn id(&self) -> u8 {
        self.id
    }

    #[cfg(any(feature = "testing", test))]
    pub fn set_max_transaction_count(&mut self, max_transaction_count: u64) {
        self.max_transaction_count = max_transaction_count;
    }

    #[cfg(any(feature = "testing", test))]
    pub fn set_max_transaction_gas_limit(&mut self, max_transaction_gas_limit: u64) {
        self.max_transaction_gas_limit = max_transaction_gas_limit;
    }

    #[cfg(any(feature = "testing", test))]
    pub fn set_max_transaction_length(&mut self, max_transaction_length: u64) {
        self.max_transaction_length = max_transaction_length;
    }
}

#[derive(Debug, Clone)]
pub enum TransactionConfigError {
    InvalidArgsProvided,
}

/// Configuration values associated with V1 Transactions.
#[derive(Clone, Eq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct TransactionV1Config {
    #[serde(
        serialize_with = "limit_definition_to_vec",
        deserialize_with = "vec_to_limit_definition"
    )]
    /// Lane configuration of the native mint interaction.
    pub native_mint_lane: TransactionLaneDefinition,
    #[serde(
        serialize_with = "limit_definition_to_vec",
        deserialize_with = "vec_to_limit_definition"
    )]
    /// Lane configuration for the native auction interaction.
    pub native_auction_lane: TransactionLaneDefinition,
    #[serde(
        serialize_with = "limit_definition_to_vec",
        deserialize_with = "vec_to_limit_definition"
    )]
    /// Lane configuration for the install/upgrade interaction.
    pub install_upgrade_lane: TransactionLaneDefinition,
    #[serde(
        serialize_with = "wasm_definitions_to_vec",
        deserialize_with = "definition_to_wasms"
    )]
    /// Lane configurations for Wasm based lanes that are not declared as install/upgrade.
    wasm_lanes: Vec<TransactionLaneDefinition>,
    #[cfg_attr(any(all(feature = "std", feature = "once_cell"), test), serde(skip))]
    #[cfg_attr(
        all(any(feature = "once_cell", test), feature = "datasize"),
        data_size(skip)
    )]
    #[cfg(any(feature = "once_cell", test))]
    wasm_lanes_ordered_by_transaction_size: OnceCell<Vec<TransactionLaneDefinition>>,
    #[cfg_attr(any(all(feature = "std", feature = "once_cell"), test), serde(skip))]
    #[cfg_attr(
        all(any(feature = "once_cell", test), feature = "datasize"),
        data_size(skip)
    )]
    #[cfg(any(feature = "once_cell", test))]
    wasm_lanes_ordered_by_transaction_gas_limit_transaction_size_args_length:
        OnceCell<Vec<TransactionLaneDefinition>>,
}

impl PartialEq for TransactionV1Config {
    fn eq(&self, other: &TransactionV1Config) -> bool {
        // Destructure to make sure we don't accidentally omit fields.
        let TransactionV1Config {
            native_mint_lane,
            native_auction_lane,
            install_upgrade_lane,
            wasm_lanes,
            #[cfg(any(feature = "once_cell", test))]
                wasm_lanes_ordered_by_transaction_size: _,
            #[cfg(any(feature = "once_cell", test))]
                wasm_lanes_ordered_by_transaction_gas_limit_transaction_size_args_length: _,
        } = self;
        *native_mint_lane == other.native_mint_lane
            && *native_auction_lane == other.native_auction_lane
            && *install_upgrade_lane == other.install_upgrade_lane
            && *wasm_lanes == other.wasm_lanes
    }
}

impl TransactionV1Config {
    /// Cretaes a new instance of TransactionV1Config
    pub fn new(
        native_mint_lane: TransactionLaneDefinition,
        native_auction_lane: TransactionLaneDefinition,
        install_upgrade_lane: TransactionLaneDefinition,
        wasm_lanes: Vec<TransactionLaneDefinition>,
    ) -> Self {
        #[cfg(any(feature = "once_cell", test))]
        let wasm_lanes_ordered_by_transaction_size = OnceCell::with_value(
            Self::build_wasm_lanes_ordered_by_transaction_size(wasm_lanes.clone()),
        );
        #[cfg(any(feature = "once_cell", test))]
        let wasm_lanes_ordered_by_transaction_gas_limit =
            OnceCell::with_value(Self::build_wasm_lanes_ordered(wasm_lanes.clone()));
        TransactionV1Config {
            native_mint_lane,
            native_auction_lane,
            install_upgrade_lane,
            wasm_lanes,
            #[cfg(any(feature = "once_cell", test))]
            wasm_lanes_ordered_by_transaction_size,
            #[cfg(any(feature = "once_cell", test))]
            wasm_lanes_ordered_by_transaction_gas_limit_transaction_size_args_length:
                wasm_lanes_ordered_by_transaction_gas_limit,
        }
    }

    #[cfg(any(feature = "testing", test))]
    /// Generates a random instance using a `TestRng`.
    pub fn random(rng: &mut TestRng) -> Self {
        let native_mint_lane = DEFAULT_NATIVE_MINT_LANE.to_vec();
        let native_auction_lane = DEFAULT_NATIVE_AUCTION_LANE.to_vec();
        let install_upgrade_lane = DEFAULT_INSTALL_UPGRADE_LANE.to_vec();
        let mut wasm_lanes = vec![];
        for kind in 2..7 {
            let lane = vec![
                kind as u64,
                rng.gen_range(0..=1_048_576),
                rng.gen_range(0..=1024),
                rng.gen_range(0..=2_500_000_000),
                rng.gen_range(5..=150),
            ];
            wasm_lanes.push(lane.try_into().unwrap())
        }

        TransactionV1Config::new(
            native_mint_lane.try_into().unwrap(),
            native_auction_lane.try_into().unwrap(),
            install_upgrade_lane.try_into().unwrap(),
            wasm_lanes,
        )
    }

    /// Returns the max serialized length of a transaction for the given lane.
    pub fn get_max_serialized_length(&self, lane_id: u8) -> u64 {
        match lane_id {
            MINT_LANE_ID => self.native_mint_lane.max_transaction_length,
            AUCTION_LANE_ID => self.native_auction_lane.max_transaction_length,
            INSTALL_UPGRADE_LANE_ID => self.install_upgrade_lane.max_transaction_length,
            _ => match self.wasm_lanes.iter().find(|lane| lane.id == lane_id) {
                Some(wasm_lane) => wasm_lane.max_transaction_length,
                None => 0,
            },
        }
    }

    /// Returns the max number of runtime args
    pub fn get_max_args_length(&self, lane_id: u8) -> u64 {
        match lane_id {
            MINT_LANE_ID => self.native_mint_lane.max_transaction_args_length,
            AUCTION_LANE_ID => self.native_auction_lane.max_transaction_args_length,
            INSTALL_UPGRADE_LANE_ID => self.install_upgrade_lane.max_transaction_args_length,
            _ => match self.wasm_lanes.iter().find(|lane| lane.id == lane_id) {
                Some(wasm_lane) => wasm_lane.max_transaction_args_length,
                None => 0,
            },
        }
    }

    /// Returns the max gas limit of a transaction for the given lane.
    pub fn get_max_transaction_gas_limit(&self, lane_id: u8) -> u64 {
        match lane_id {
            MINT_LANE_ID => self.native_mint_lane.max_transaction_gas_limit,
            AUCTION_LANE_ID => self.native_auction_lane.max_transaction_gas_limit,
            INSTALL_UPGRADE_LANE_ID => self.install_upgrade_lane.max_transaction_gas_limit,
            _ => match self.wasm_lanes.iter().find(|lane| lane.id == lane_id) {
                Some(wasm_lane) => wasm_lane.max_transaction_gas_limit,
                None => 0,
            },
        }
    }

    /// Returns the max transactions count for the given lane.
    pub fn get_max_transaction_count(&self, lane_id: u8) -> u64 {
        match lane_id {
            MINT_LANE_ID => self.native_mint_lane.max_transaction_count,
            AUCTION_LANE_ID => self.native_auction_lane.max_transaction_count,
            INSTALL_UPGRADE_LANE_ID => self.install_upgrade_lane.max_transaction_count,
            _ => match self.wasm_lanes.iter().find(|lane| lane.id == lane_id) {
                Some(wasm_lane) => wasm_lane.max_transaction_count,
                None => 0,
            },
        }
    }

    /// Returns the maximum number of Wasm based transactions across wasm lanes.
    pub fn get_max_wasm_transaction_count(&self) -> u64 {
        let mut ret = 0;
        for lane in self.wasm_lanes.iter() {
            ret += lane.max_transaction_count;
        }
        ret
    }

    /// Are the given transaction parameters supported.
    pub fn is_supported(&self, lane_id: u8) -> bool {
        if !self.is_predefined_lane(lane_id) {
            return self.wasm_lanes.iter().any(|lane| lane.id == lane_id);
        }
        true
    }

    /// Returns the list of currently supported lane identifiers.
    pub fn get_supported_lanes(&self) -> Vec<u8> {
        let mut ret = vec![0, 1, 2];
        for lane in self.wasm_lanes.iter() {
            ret.push(lane.id);
        }
        ret
    }

    /// Returns the transaction v1 configuration with the specified lane limits.
    #[cfg(any(feature = "testing", test))]
    pub fn with_count_limits(
        mut self,
        mint: Option<u64>,
        auction: Option<u64>,
        install: Option<u64>,
        large: Option<u64>,
    ) -> Self {
        if let Some(mint_count) = mint {
            self.native_mint_lane.max_transaction_count = mint_count;
        }
        if let Some(auction_count) = auction {
            self.native_auction_lane.max_transaction_count = auction_count;
        }
        if let Some(install_upgrade) = install {
            self.install_upgrade_lane.max_transaction_count = install_upgrade;
        }
        if let Some(large_limit) = large {
            for lane in self.wasm_lanes.iter_mut() {
                if lane.id == 3 {
                    lane.max_transaction_count = large_limit;
                }
            }
        }
        self
    }

    /// Returns the max total count for all transactions across all lanes allowed in a block.
    pub fn get_max_block_count(&self) -> u64 {
        self.native_mint_lane.max_transaction_count
            + self.native_auction_lane.max_transaction_count
            + self.install_upgrade_lane.max_transaction_count
            + self
                .wasm_lanes
                .iter()
                .map(|lane| lane.max_transaction_count)
                .sum::<u64>()
    }

    /// Returns true if the lane identifier is for one of the predefined lanes.
    pub fn is_predefined_lane(&self, lane: u8) -> bool {
        lane == AUCTION_LANE_ID || lane == MINT_LANE_ID || lane == INSTALL_UPGRADE_LANE_ID
    }

    /// Returns a wasm lane id based on the transaction size adjusted by
    /// maybe_additional_computation_factor if necessary.
    pub fn get_wasm_lane_id_by_size(
        &self,
        transaction_size: u64,
        additional_computation_factor: u8,
        runtime_args_size: u64,
    ) -> Option<u8> {
        let mut maybe_adequate_lane_index = None;
        let buckets = self.get_wasm_lanes_ordered_by_transaction_size();
        let number_of_lanes = buckets.len();
        for (i, lane) in buckets.iter().enumerate() {
            let max_transaction_size = lane.max_transaction_length;
            let max_runtime_args_size = lane.max_transaction_args_length;
            if max_transaction_size >= transaction_size
                && max_runtime_args_size >= runtime_args_size
            {
                maybe_adequate_lane_index = Some(i);
                break;
            }
        }
        if let Some(adequate_lane_index) = maybe_adequate_lane_index {
            maybe_adequate_lane_index = Some(cmp::min(
                adequate_lane_index + additional_computation_factor as usize,
                number_of_lanes - 1,
            ));
        }
        maybe_adequate_lane_index.map(|index| buckets[index].id)
    }

    pub fn get_lane_by_id(&self, lane_id: u8) -> Option<&TransactionLaneDefinition> {
        if lane_id == MINT_LANE_ID {
            return Some(&self.native_mint_lane);
        }
        if lane_id == AUCTION_LANE_ID {
            return Some(&self.native_auction_lane);
        }
        if lane_id == INSTALL_UPGRADE_LANE_ID {
            return Some(&self.install_upgrade_lane);
        }
        self.wasm_lanes.iter().find(|el| el.id == lane_id)
    }

    pub fn get_wasm_lane_id_by_payment_limited(
        &self,
        gas_limit: u64,
        transaction_size: u64,
        runtime_args_size: u64,
    ) -> Option<u8> {
        let mut maybe_adequate_lane_index = None;
        let lanes = self.get_wasm_lanes_ordered();
        for (i, lane) in lanes.iter().enumerate() {
            let max_transaction_gas = lane.max_transaction_gas_limit;
            let max_transaction_size = lane.max_transaction_length;
            let max_runtime_args_size = lane.max_transaction_args_length;
            if gas_limit <= max_transaction_gas
                && transaction_size <= max_transaction_size
                && runtime_args_size <= max_runtime_args_size
            {
                maybe_adequate_lane_index = Some(i);
                break;
            }
        }
        maybe_adequate_lane_index.map(|index| lanes[index].id)
    }

    #[allow(unreachable_code)]
    //We're allowing unreachable code here because there's a possibility that someone might
    // want to use the types crate without once_cell
    fn get_wasm_lanes_ordered_by_transaction_size(&self) -> &Vec<TransactionLaneDefinition> {
        #[cfg(any(feature = "once_cell", test))]
        return self.wasm_lanes_ordered_by_transaction_size.get_or_init(|| {
            Self::build_wasm_lanes_ordered_by_transaction_size(self.wasm_lanes.clone())
        });
        &Self::build_wasm_lanes_ordered_by_transaction_size(self.wasm_lanes.clone())
    }

    #[allow(unreachable_code)]
    //We're allowing unreachable code here because there's a possibility that someone might
    // want to use the types crate without once_cell
    // This function will take the wasm lanes ordered by:
    //   - firstly gas limit
    //   - secondly max_transaction_length
    //   - thirdly max runtime args
    //   - fourthly lane id (this has no "business" value, but it ensures that the ordering is
    //     always reproducible since ids should be unique)
    fn get_wasm_lanes_ordered(&self) -> &Vec<TransactionLaneDefinition> {
        #[cfg(any(feature = "once_cell", test))]
        return self
            .wasm_lanes_ordered_by_transaction_gas_limit_transaction_size_args_length
            .get_or_init(|| Self::build_wasm_lanes_ordered(self.wasm_lanes.clone()));
        &Self::build_wasm_lanes_ordered(self.wasm_lanes.clone())
    }

    fn build_wasm_lanes_ordered(
        wasm_lanes: Vec<TransactionLaneDefinition>,
    ) -> Vec<TransactionLaneDefinition> {
        let mut ordered = wasm_lanes;
        ordered.sort_by_key(|item| {
            (
                item.max_transaction_gas_limit,
                item.max_transaction_length,
                item.max_transaction_args_length,
                item.id,
            )
        });
        ordered
    }

    fn build_wasm_lanes_ordered_by_transaction_size(
        wasm_lanes: Vec<TransactionLaneDefinition>,
    ) -> Vec<TransactionLaneDefinition> {
        let mut ordered = wasm_lanes;
        ordered.sort_by(|a, b| a.max_transaction_length.cmp(&b.max_transaction_length));
        ordered
    }

    pub fn wasm_lanes(&self) -> &Vec<TransactionLaneDefinition> {
        &self.wasm_lanes
    }

    #[cfg(any(feature = "testing", test))]
    pub fn set_wasm_lanes(&mut self, wasm_lanes: Vec<TransactionLaneDefinition>) {
        self.wasm_lanes = wasm_lanes;
        #[cfg(any(feature = "once_cell", test))]
        {
            let wasm_lanes_ordered_by_transaction_size = OnceCell::with_value(
                Self::build_wasm_lanes_ordered_by_transaction_size(self.wasm_lanes.clone()),
            );
            self.wasm_lanes_ordered_by_transaction_size = wasm_lanes_ordered_by_transaction_size;
            let wasm_lanes_ordered_by_transaction_gas_limit =
                OnceCell::with_value(Self::build_wasm_lanes_ordered(self.wasm_lanes.clone()));
            self.wasm_lanes_ordered_by_transaction_gas_limit_transaction_size_args_length =
                wasm_lanes_ordered_by_transaction_gas_limit;
        }
    }

    #[cfg(any(feature = "testing", test))]
    pub fn get_max_wasm_lane_by_gas_limit(&self) -> Option<TransactionLaneDefinition> {
        self.wasm_lanes
            .iter()
            .max_by(|a, b| {
                a.max_transaction_gas_limit
                    .cmp(&b.max_transaction_gas_limit)
            })
            .cloned()
    }

    #[cfg(any(feature = "testing", test))]
    pub fn very_big_wasm_lane() -> Self {
        let mut default = Self::default();
        // Using `set_wasm_lanes` to make sure caches are refreshed
        default.set_wasm_lanes(
            // The division here is to avoid overflowing when
            // adding capacity of lanes
            vec![TransactionLaneDefinition::new(
                15,
                u64::MAX / 5,
                u64::MAX / 5,
                u64::MAX / 5,
                100,
            )],
        );
        default
    }
}

#[cfg(any(feature = "std", test))]
impl Default for TransactionV1Config {
    fn default() -> Self {
        let wasm_lane = vec![
            3_u64, //large lane id
            1_048_576,
            1024,
            DEFAULT_LARGE_TRANSACTION_GAS_LIMIT,
            10,
        ];

        let native_mint_lane = DEFAULT_NATIVE_MINT_LANE.to_vec();
        let native_auction_lane = DEFAULT_NATIVE_AUCTION_LANE.to_vec();
        let install_upgrade_lane = DEFAULT_INSTALL_UPGRADE_LANE.to_vec();
        let raw_wasm_lanes = vec![wasm_lane];
        let wasm_lanes: Result<Vec<TransactionLaneDefinition>, _> =
            raw_wasm_lanes.into_iter().map(|v| v.try_into()).collect();

        TransactionV1Config::new(
            native_mint_lane.try_into().unwrap(),
            native_auction_lane.try_into().unwrap(),
            install_upgrade_lane.try_into().unwrap(),
            wasm_lanes.unwrap(),
        )
    }
}

impl ToBytes for TransactionV1Config {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.native_mint_lane.as_vec().write_bytes(writer)?;
        self.native_auction_lane.as_vec().write_bytes(writer)?;
        self.install_upgrade_lane.as_vec().write_bytes(writer)?;
        let wasm_lanes_as_vecs: Vec<Vec<u64>> = self
            .wasm_lanes
            .iter()
            .map(TransactionLaneDefinition::as_vec)
            .collect();
        wasm_lanes_as_vecs.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        let wasm_lanes_as_vecs: Vec<Vec<u64>> = self
            .wasm_lanes
            .iter()
            .map(TransactionLaneDefinition::as_vec)
            .collect();
        self.native_mint_lane.as_vec().serialized_length()
            + self.native_auction_lane.as_vec().serialized_length()
            + self.install_upgrade_lane.as_vec().serialized_length()
            + wasm_lanes_as_vecs.serialized_length()
    }
}

impl FromBytes for TransactionV1Config {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (raw_native_mint_lane, remainder): (Vec<u64>, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (raw_native_auction_lane, remainder): (Vec<u64>, &[u8]) =
            FromBytes::from_bytes(remainder)?;
        let (raw_install_upgrade_lane, remainder): (Vec<u64>, &[u8]) =
            FromBytes::from_bytes(remainder)?;
        let (raw_wasm_lanes, remainder): (Vec<Vec<u64>>, &[u8]) = FromBytes::from_bytes(remainder)?;
        let native_mint_lane = raw_native_mint_lane
            .try_into()
            .map_err(|_| bytesrepr::Error::Formatting)?;
        let native_auction_lane = raw_native_auction_lane
            .try_into()
            .map_err(|_| bytesrepr::Error::Formatting)?;
        let install_upgrade_lane = raw_install_upgrade_lane
            .try_into()
            .map_err(|_| bytesrepr::Error::Formatting)?;
        let wasm_lanes: Result<Vec<TransactionLaneDefinition>, _> =
            raw_wasm_lanes.into_iter().map(|v| v.try_into()).collect();
        let config = TransactionV1Config::new(
            native_mint_lane,
            native_auction_lane,
            install_upgrade_lane,
            wasm_lanes.map_err(|_| bytesrepr::Error::Formatting)?,
        );
        Ok((config, remainder))
    }
}

fn vec_to_limit_definition<'de, D>(deserializer: D) -> Result<TransactionLaneDefinition, D::Error>
where
    D: Deserializer<'de>,
{
    let vec = Vec::<u64>::deserialize(deserializer)?;
    let limits = TransactionLaneDefinition::try_from(vec).map_err(|_| {
        D::Error::invalid_value(
            Unexpected::Seq,
            &"expected 5 u64 compliant numbers to create a TransactionLimitsDefinition",
        )
    })?;
    Ok(limits)
}

fn limit_definition_to_vec<S>(
    limits: &TransactionLaneDefinition,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let vec = limits.as_vec();
    let mut seq = serializer.serialize_seq(Some(vec.len()))?;
    for element in vec {
        seq.serialize_element(&element)?;
    }
    seq.end()
}

fn definition_to_wasms<'de, D>(deserializer: D) -> Result<Vec<TransactionLaneDefinition>, D::Error>
where
    D: Deserializer<'de>,
{
    let vec = Vec::<Vec<u64>>::deserialize(deserializer)?;
    let result: Result<Vec<TransactionLaneDefinition>, TransactionConfigError> =
        vec.into_iter().map(|v| v.try_into()).collect();
    result.map_err(|_| {
        D::Error::invalid_value(
            Unexpected::Seq,
            &"sequence of sequences to assemble wasm definitions",
        )
    })
}

fn wasm_definitions_to_vec<S>(
    limits: &[TransactionLaneDefinition],
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let vec_of_vecs: Vec<Vec<u64>> = limits.iter().map(|v| v.as_vec()).collect();
    let mut seq = serializer.serialize_seq(Some(vec_of_vecs.len()))?;
    for element in vec_of_vecs {
        seq.serialize_element(&element)?;
    }
    seq.end()
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;
    const EXAMPLE_JSON: &str = r#"{
        "native_mint_lane": [0,1,2,3,4],
        "native_auction_lane": [1,5,6,7,8],
        "install_upgrade_lane": [2,9,10,11,12],
        "wasm_lanes": [[3,13,14,15,16], [4,17,18,19,20], [5,21,22,23,24]]
        }"#;
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
        assert!(config.is_supported(2));
        assert!(config.is_supported(3));
        assert!(!config.is_supported(10));
    }

    #[test]
    fn should_get_configuration_for_wasm() {
        let config = build_example_transaction_config();
        let got = config.get_wasm_lane_id_by_size(100, 0, 0);
        assert_eq!(got, Some(3));
        let config = build_example_transaction_config_reverse_wasm_ids();
        let got = config.get_wasm_lane_id_by_size(100, 0, 0);
        assert_eq!(got, Some(5));
    }

    #[test]
    fn given_too_big_transaction_should_return_none() {
        let config = build_example_transaction_config();
        let got = config.get_wasm_lane_id_by_size(100000000, 0, 0);
        assert!(got.is_none());
        let config = build_example_transaction_config_reverse_wasm_ids();
        let got = config.get_wasm_lane_id_by_size(100000000, 0, 0);
        assert!(got.is_none());
        let config = build_example_transaction_config_reverse_wasm_ids();
        let got = config.get_wasm_lane_id_by_size(1, 0, 100000);
        assert!(got.is_none());
    }

    #[test]
    fn given_wasm_should_return_first_fit() {
        let config = build_example_transaction_config();

        let got = config.get_wasm_lane_id_by_size(660, 0, 0);
        assert_eq!(got, Some(4));

        let got = config.get_wasm_lane_id_by_size(800, 0, 0);
        assert_eq!(got, Some(5));

        let got = config.get_wasm_lane_id_by_size(1, 0, 0);
        assert_eq!(got, Some(3));

        let got = config.get_wasm_lane_id_by_size(800, 0, 6024);
        assert_eq!(got, Some(5));

        let config = build_example_transaction_config_reverse_wasm_ids();

        let got = config.get_wasm_lane_id_by_size(660, 0, 0);
        assert_eq!(got, Some(4));

        let got = config.get_wasm_lane_id_by_size(800, 0, 0);
        assert_eq!(got, Some(3));

        let got = config.get_wasm_lane_id_by_size(1, 0, 0);
        assert_eq!(got, Some(5));

        let got = config.get_wasm_lane_id_by_size(800, 0, 6024);
        assert_eq!(got, Some(3));
    }

    #[test]
    fn given_additional_computation_factor_should_be_applied() {
        let config = build_example_transaction_config();
        let got = config.get_wasm_lane_id_by_size(660, 1, 0);
        assert_eq!(got, Some(5));

        let config = build_example_transaction_config_reverse_wasm_ids();
        let got = config.get_wasm_lane_id_by_size(660, 1, 0);
        assert_eq!(got, Some(3));
    }

    #[test]
    fn given_additional_computation_factor_should_not_overflow() {
        let config = build_example_transaction_config();
        let got = config.get_wasm_lane_id_by_size(660, 2, 0);
        assert_eq!(got, Some(5));
        let got_2 = config.get_wasm_lane_id_by_size(660, 20, 0);
        assert_eq!(got_2, Some(5));

        let config = build_example_transaction_config_reverse_wasm_ids();
        let got = config.get_wasm_lane_id_by_size(660, 2, 0);
        assert_eq!(got, Some(3));
        let got_2 = config.get_wasm_lane_id_by_size(660, 20, 0);
        assert_eq!(got_2, Some(3));
    }

    #[test]
    fn given_no_wasm_lanes_should_return_none() {
        let config = build_example_transaction_config_no_wasms();
        let got = config.get_wasm_lane_id_by_size(660, 2, 0);
        assert!(got.is_none());
        let got = config.get_wasm_lane_id_by_size(660, 0, 0);
        assert!(got.is_none());
        let got = config.get_wasm_lane_id_by_size(660, 20, 0);
        assert!(got.is_none());

        let got = config.get_wasm_lane_id_by_payment_limited(100, 1, 0);
        assert!(got.is_none());
    }

    #[test]
    fn given_wasm_when_by_payment_should_find_smallest_lane() {
        let config = TransactionV1Config::new(
            example_native(),
            example_auction(),
            example_install_upgrade(),
            vec![
                TransactionLaneDefinition {
                    id: 3,
                    max_transaction_length: 10,
                    max_transaction_args_length: 1,
                    max_transaction_gas_limit: 5,
                    max_transaction_count: 1,
                },
                TransactionLaneDefinition {
                    id: 4,
                    max_transaction_length: 11,
                    max_transaction_args_length: 1,
                    max_transaction_gas_limit: 55,
                    max_transaction_count: 1,
                },
                TransactionLaneDefinition {
                    id: 5,
                    max_transaction_length: 12,
                    max_transaction_args_length: 5,
                    max_transaction_gas_limit: 155,
                    max_transaction_count: 1,
                },
            ],
        );
        let got = config.get_wasm_lane_id_by_payment_limited(54, 1, 0);
        assert_eq!(got, Some(4));
        let got = config.get_wasm_lane_id_by_payment_limited(54, 10, 3);
        assert_eq!(got, Some(5));
    }

    #[test]
    fn given_wasm_when_by_payment_should_take_size_into_consideration() {
        let config = TransactionV1Config::new(
            example_native(),
            example_auction(),
            example_install_upgrade(),
            vec![
                TransactionLaneDefinition {
                    id: 3,
                    max_transaction_length: 10,
                    max_transaction_args_length: 1,
                    max_transaction_gas_limit: 5,
                    max_transaction_count: 1,
                },
                TransactionLaneDefinition {
                    id: 4,
                    max_transaction_length: 11,
                    max_transaction_args_length: 1,
                    max_transaction_gas_limit: 55,
                    max_transaction_count: 1,
                },
                TransactionLaneDefinition {
                    id: 5,
                    max_transaction_length: 12,
                    max_transaction_args_length: 1,
                    max_transaction_gas_limit: 155,
                    max_transaction_count: 1,
                },
            ],
        );
        let got = config.get_wasm_lane_id_by_payment_limited(54, 12, 0);
        assert_eq!(got, Some(5));
    }

    #[test]
    fn given_wasm_when_by_payment_should_return_none_if_no_size_fits() {
        let config = TransactionV1Config::new(
            example_native(),
            example_auction(),
            example_install_upgrade(),
            vec![
                TransactionLaneDefinition {
                    id: 3,
                    max_transaction_length: 10,
                    max_transaction_args_length: 1,
                    max_transaction_gas_limit: 5,
                    max_transaction_count: 1,
                },
                TransactionLaneDefinition {
                    id: 4,
                    max_transaction_length: 11,
                    max_transaction_args_length: 1,
                    max_transaction_gas_limit: 55,
                    max_transaction_count: 1,
                },
                TransactionLaneDefinition {
                    id: 5,
                    max_transaction_length: 12,
                    max_transaction_args_length: 5,
                    max_transaction_gas_limit: 155,
                    max_transaction_count: 1,
                },
            ],
        );
        let got = config.get_wasm_lane_id_by_payment_limited(54, 120, 0);
        assert_eq!(got, None);
        let got = config.get_wasm_lane_id_by_payment_limited(54, 10, 1000);
        assert_eq!(got, None);
    }

    #[test]
    fn should_deserialize() {
        let got: TransactionV1Config = serde_json::from_str(EXAMPLE_JSON).unwrap();
        let expected = TransactionV1Config::new(
            TransactionLaneDefinition::new(0, 1, 2, 3, 4),
            TransactionLaneDefinition::new(1, 5, 6, 7, 8),
            TransactionLaneDefinition::new(2, 9, 10, 11, 12),
            vec![
                TransactionLaneDefinition::new(3, 13, 14, 15, 16),
                TransactionLaneDefinition::new(4, 17, 18, 19, 20),
                TransactionLaneDefinition::new(5, 21, 22, 23, 24),
            ],
        );
        assert_eq!(got, expected);
    }

    #[test]
    fn should_serialize() {
        let input = TransactionV1Config::new(
            TransactionLaneDefinition::new(0, 1, 2, 3, 4),
            TransactionLaneDefinition::new(1, 5, 6, 7, 8),
            TransactionLaneDefinition::new(2, 9, 10, 11, 12),
            vec![
                TransactionLaneDefinition::new(3, 13, 14, 15, 16),
                TransactionLaneDefinition::new(4, 17, 18, 19, 20),
                TransactionLaneDefinition::new(5, 21, 22, 23, 24),
            ],
        );
        let raw = serde_json::to_string(&input).unwrap();
        let got = serde_json::from_str::<Value>(&raw).unwrap();
        let expected: Value = serde_json::from_str::<Value>(EXAMPLE_JSON).unwrap();
        assert_eq!(got, expected);
    }

    #[test]
    fn should_order_by_nested_predicates() {
        // Firstly, order by max_transaction_gas_limit
        let definition_1 = TransactionLaneDefinition::new(0, 0, 0, 4, 0);
        let definition_2 = TransactionLaneDefinition::new(1, 0, 0, 3, 0);
        let definition_3 = TransactionLaneDefinition::new(2, 0, 0, 2, 0);
        let res = TransactionV1Config::build_wasm_lanes_ordered(vec![
            definition_1.clone(),
            definition_2.clone(),
            definition_3.clone(),
        ]);
        assert_eq!(res, vec![definition_3, definition_2, definition_1,]);

        // If max_transaction_gas_limit equal, order by
        let definition_1 = TransactionLaneDefinition::new(0, 3, 0, 1, 0);
        let definition_2 = TransactionLaneDefinition::new(1, 4, 0, 1, 0);
        let definition_3 = TransactionLaneDefinition::new(2, 2, 0, 1, 0);
        let res = TransactionV1Config::build_wasm_lanes_ordered(vec![
            definition_1.clone(),
            definition_2.clone(),
            definition_3.clone(),
        ]);
        assert_eq!(res, vec![definition_3, definition_1, definition_2,]);

        // If max_transaction_gas_limit and max_transaction_length equal, order by
        // max_transaction_args_length
        let definition_1 = TransactionLaneDefinition::new(0, 2, 4, 1, 0);
        let definition_2 = TransactionLaneDefinition::new(1, 2, 2, 1, 0);
        let definition_3 = TransactionLaneDefinition::new(2, 2, 3, 1, 0);
        let res = TransactionV1Config::build_wasm_lanes_ordered(vec![
            definition_1.clone(),
            definition_2.clone(),
            definition_3.clone(),
        ]);
        assert_eq!(res, vec![definition_2, definition_3, definition_1,]);

        // If max_transaction_gas_limit and max_transaction_length equal and
        // max_transaction_args_length, order by id
        let definition_1 = TransactionLaneDefinition::new(2, 2, 3, 1, 0);
        let definition_2 = TransactionLaneDefinition::new(0, 2, 3, 1, 0);
        let definition_3 = TransactionLaneDefinition::new(1, 2, 3, 1, 0);
        let res = TransactionV1Config::build_wasm_lanes_ordered(vec![
            definition_2.clone(),
            definition_3.clone(),
            definition_1.clone(),
        ]);
        assert_eq!(res, vec![definition_2, definition_3, definition_1,]);

        // Should apply those rules mixed
        let definition_1 = TransactionLaneDefinition::new(10, 0, 2, 2, 0);
        let definition_2 = TransactionLaneDefinition::new(1, 2, 3, 1, 0);
        let definition_3 = TransactionLaneDefinition::new(2, 4, 3, 1, 0);
        let definition_4 = TransactionLaneDefinition::new(3, 4, 2, 1, 0);
        let definition_5 = TransactionLaneDefinition::new(4, 0, 0, 2, 0);
        let definition_6 = TransactionLaneDefinition::new(5, 4, 3, 1, 0);

        let res = TransactionV1Config::build_wasm_lanes_ordered(vec![
            definition_1.clone(),
            definition_2.clone(),
            definition_3.clone(),
            definition_4.clone(),
            definition_5.clone(),
            definition_6.clone(),
        ]);
        assert_eq!(
            res,
            vec![
                definition_2,
                definition_4,
                definition_3,
                definition_6,
                definition_5,
                definition_1
            ]
        );
    }

    fn example_native() -> TransactionLaneDefinition {
        TransactionLaneDefinition::new(0, 1500, 1024, 1_500_000_000, 150)
    }

    fn example_auction() -> TransactionLaneDefinition {
        TransactionLaneDefinition::new(1, 500, 3024, 3_500_000_000, 350)
    }

    fn example_install_upgrade() -> TransactionLaneDefinition {
        TransactionLaneDefinition::new(2, 10000, 2024, 2_500_000_000, 250)
    }

    fn wasm_small(id: u8) -> TransactionLaneDefinition {
        TransactionLaneDefinition::new(id, 600, 4024, 4_500_000_000, 450)
    }

    fn wasm_medium(id: u8) -> TransactionLaneDefinition {
        TransactionLaneDefinition::new(id, 700, 5024, 5_500_000_000, 550)
    }

    fn wasm_large(id: u8) -> TransactionLaneDefinition {
        TransactionLaneDefinition::new(id, 800, 6024, 6_500_000_000, 650)
    }

    fn example_wasm() -> Vec<TransactionLaneDefinition> {
        vec![wasm_small(3), wasm_medium(4), wasm_large(5)]
    }

    fn example_wasm_reversed_ids() -> Vec<TransactionLaneDefinition> {
        vec![wasm_small(5), wasm_medium(4), wasm_large(3)]
    }

    fn build_example_transaction_config_no_wasms() -> TransactionV1Config {
        TransactionV1Config::new(
            example_native(),
            example_auction(),
            example_install_upgrade(),
            vec![],
        )
    }

    fn build_example_transaction_config() -> TransactionV1Config {
        TransactionV1Config::new(
            example_native(),
            example_auction(),
            example_install_upgrade(),
            example_wasm(),
        )
    }

    fn build_example_transaction_config_reverse_wasm_ids() -> TransactionV1Config {
        TransactionV1Config::new(
            example_native(),
            example_auction(),
            example_install_upgrade(),
            example_wasm_reversed_ids(),
        )
    }
}
