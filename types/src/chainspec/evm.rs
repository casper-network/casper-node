use alloc::{collections::BTreeMap, vec::Vec};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
#[cfg(any(feature = "std", test))]
use serde::{
    de::{Error as DeError, Unexpected},
    Deserializer, Serializer,
};
use serde::{Deserialize, Serialize};

use super::TransactionLaneDefinition;
use crate::{
    bytesrepr::{self, Bytes, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    EvmFeeConfig, U256, U512,
};

/// The default number of wei represented by one mote.
pub const DEFAULT_WEI_PER_MOTE: u64 = 1_000_000_000;

/// The minimum number of wei represented by one mote.
pub const MINIMUM_WEI_PER_MOTE: u64 = DEFAULT_WEI_PER_MOTE;

/// Supported EVM hardfork specifications for chainspec configuration.
///
/// Variants are ordered by fork chronology; feature gates use ordinal
/// comparisons to apply functionality from a fork onward.
#[derive(
    Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default, Serialize, Deserialize,
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum EvmSpec {
    /// Prague.
    #[default]
    Prague,
}

impl EvmSpec {
    fn tag(self) -> u8 {
        match self {
            EvmSpec::Prague => 0,
        }
    }
}

impl ToBytes for EvmSpec {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        Ok(vec![self.tag()])
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        writer.push(self.tag());
        Ok(())
    }
}

impl FromBytes for EvmSpec {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        let spec = match tag {
            0 => EvmSpec::Prague,
            _ => return Err(bytesrepr::Error::Formatting),
        };
        Ok((spec, remainder))
    }
}

/// Chainspec configuration for EVM execution.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct EvmConfig {
    /// Whether EVM execution is enabled.
    pub enabled: bool,
    /// EVM chain ID used for the `CHAINID` opcode and transaction validation.
    pub chain_id: u64,
    /// Hardfork specification used by the EVM executor.
    pub spec: EvmSpec,
    /// Per-block gas limit supplied to the EVM block context.
    pub block_gas_limit: u64,
    /// Base fee denominated in motes per EVM gas.
    pub base_fee: u64,
    /// Number of wei represented by one mote.
    pub wei_per_mote: u64,
    /// Lane configurations for EVM transactions.
    #[serde(
        serialize_with = "transaction_lane_definitions_to_vec",
        deserialize_with = "vec_to_transaction_lane_definitions"
    )]
    pub transaction_lanes: Vec<TransactionLaneDefinition>,
    /// Runtime bytecode installed at the configured addresses at genesis and upgrades.
    ///
    /// TOML uses an `[evm.preinstalls]` address-to-bytecode table. Both keys and
    /// values are 0x-prefixed hexadecimal strings. An omitted table is empty.
    #[serde(default, with = "super::evm_preinstalls")]
    #[cfg_attr(
        feature = "json-schema",
        schemars(with = "BTreeMap<crate::evm::Address, alloc::string::String>")
    )]
    pub preinstalls: BTreeMap<crate::evm::Address, Bytes>,
}

impl Default for EvmConfig {
    fn default() -> Self {
        EvmConfig {
            enabled: false,
            chain_id: 0,
            spec: EvmSpec::Prague,
            block_gas_limit: 30_000_000,
            base_fee: 0,
            wei_per_mote: DEFAULT_WEI_PER_MOTE,
            transaction_lanes: Vec::new(),
            preinstalls: BTreeMap::new(),
        }
    }
}

#[cfg(any(feature = "std", test))]
fn transaction_lane_definition_to_vec(lane: &TransactionLaneDefinition) -> Vec<u64> {
    vec![
        lane.id() as u64,
        lane.max_transaction_length(),
        lane.max_transaction_args_length(),
        lane.max_transaction_gas_limit(),
        lane.max_transaction_count(),
    ]
}

#[cfg(any(feature = "std", test))]
fn transaction_lane_definitions_to_vec<S>(
    lanes: &[TransactionLaneDefinition],
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let as_vecs: Vec<Vec<u64>> = lanes
        .iter()
        .map(transaction_lane_definition_to_vec)
        .collect();
    as_vecs.serialize(serializer)
}

#[cfg(any(feature = "std", test))]
fn vec_to_transaction_lane_definitions<'de, D>(
    deserializer: D,
) -> Result<Vec<TransactionLaneDefinition>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw_lanes = Vec::<Vec<u64>>::deserialize(deserializer)?;
    raw_lanes
        .into_iter()
        .map(|v| {
            TransactionLaneDefinition::try_from(v).map_err(|_| {
                DeError::invalid_value(
                    Unexpected::Seq,
                    &"expected 5 u64 compliant numbers to create a TransactionLaneDefinition",
                )
            })
        })
        .collect()
}

impl EvmConfig {
    /// Returns the subset of this configuration needed to price EVM transactions.
    pub fn fee_config(&self) -> EvmFeeConfig {
        EvmFeeConfig::new(self.base_fee, self.wei_per_mote)
    }

    /// Returns the EVM base fee denominated in wei.
    pub fn base_fee_wei(&self) -> u128 {
        self.fee_config().base_fee_wei()
    }

    /// Converts an EVM gas cost, denominated in wei, to motes by rounding up.
    ///
    /// See [`EvmFeeConfig::gas_fee_motes`].
    pub fn gas_fee_motes(&self, gas: u64, gas_price_wei: u128) -> Option<U512> {
        self.fee_config().gas_fee_motes(gas, gas_price_wei)
    }

    /// Converts an Ethereum transaction value from wei to motes.
    ///
    /// See [`EvmFeeConfig::value_motes`].
    pub fn value_motes(&self, value_wei: U256) -> Option<U256> {
        self.fee_config().value_motes(value_wei)
    }
}

// EVM transaction lanes reuse `TransactionLaneDefinition`, which lives in the `chainspec`.
impl EvmConfig {
    /// Returns the configured EVM transaction lanes.
    pub fn transaction_lanes(&self) -> &Vec<TransactionLaneDefinition> {
        &self.transaction_lanes
    }

    /// Sets the configured EVM transaction lanes.
    #[cfg(any(feature = "testing", test))]
    pub fn set_transaction_lanes(&mut self, transaction_lanes: Vec<TransactionLaneDefinition>) {
        self.transaction_lanes = transaction_lanes;
    }

    /// Returns the lane definition matching the given lane id, if any.
    pub fn get_lane_by_id(&self, lane_id: u8) -> Option<&TransactionLaneDefinition> {
        self.transaction_lanes
            .iter()
            .find(|lane| lane.id() == lane_id)
    }

    /// Returns the max serialized length of a transaction for the given EVM lane.
    pub fn get_max_serialized_length(&self, lane_id: u8) -> u64 {
        self.get_lane_by_id(lane_id)
            .map(TransactionLaneDefinition::max_transaction_length)
            .unwrap_or(0)
    }

    /// Returns the max calldata (args) length of a transaction for the given EVM lane.
    pub fn get_max_args_length(&self, lane_id: u8) -> u64 {
        self.get_lane_by_id(lane_id)
            .map(TransactionLaneDefinition::max_transaction_args_length)
            .unwrap_or(0)
    }

    /// Returns the max gas limit of a transaction for the given EVM lane.
    pub fn get_max_transaction_gas_limit(&self, lane_id: u8) -> u64 {
        self.get_lane_by_id(lane_id)
            .map(TransactionLaneDefinition::max_transaction_gas_limit)
            .unwrap_or(0)
    }

    /// Returns the max transaction count for the given EVM lane.
    pub fn get_max_transaction_count(&self, lane_id: u8) -> u64 {
        self.get_lane_by_id(lane_id)
            .map(TransactionLaneDefinition::max_transaction_count)
            .unwrap_or(0)
    }

    /// Returns the maximum number of EVM transactions across all configured EVM lanes.
    pub fn get_max_evm_transaction_count(&self) -> Option<u64> {
        if !self.enabled {
            return None;
        }
        Some(
            self.transaction_lanes
                .iter()
                .map(TransactionLaneDefinition::max_transaction_count)
                .sum(),
        )
    }

    /// Is the given EVM lane identifier supported.
    pub fn is_supported(&self, lane_id: u8) -> bool {
        self.transaction_lanes
            .iter()
            .any(|lane| lane.id() == lane_id)
    }

    /// Returns the list of currently supported EVM lane identifiers.
    pub fn get_supported_lanes(&self) -> Vec<u8> {
        self.transaction_lanes
            .iter()
            .map(TransactionLaneDefinition::id)
            .collect()
    }

    /// Returns the smallest EVM lane id whose limits can accommodate a transaction with the
    /// given gas limit, serialized size and calldata (input) size. Lanes are considered in
    /// ascending order of `(max_transaction_gas_limit, max_transaction_length,
    /// max_transaction_args_length, id)`, mirroring
    /// `TransactionV1Config::get_wasm_lane_id_by_payment_limited`.
    pub fn get_evm_lane_id(
        &self,
        gas_limit: u64,
        transaction_size: u64,
        input_size: u64,
    ) -> Option<u8> {
        let mut lanes: Vec<&TransactionLaneDefinition> = self.transaction_lanes.iter().collect();
        lanes.sort_by_key(|lane| {
            (
                lane.max_transaction_gas_limit(),
                lane.max_transaction_length(),
                lane.max_transaction_args_length(),
                lane.id(),
            )
        });
        lanes
            .into_iter()
            .find(|lane| {
                gas_limit <= lane.max_transaction_gas_limit()
                    && transaction_size <= lane.max_transaction_length()
                    && input_size <= lane.max_transaction_args_length()
            })
            .map(TransactionLaneDefinition::id)
    }
}

impl ToBytes for EvmConfig {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        let base = self.enabled.serialized_length()
            + self.chain_id.serialized_length()
            + self.spec.serialized_length()
            + self.block_gas_limit.serialized_length()
            + self.base_fee.serialized_length()
            + self.wei_per_mote.serialized_length();
        let base = {
            let transaction_lanes_as_vecs: Vec<Vec<u64>> = self
                .transaction_lanes
                .iter()
                .map(transaction_lane_definition_to_vec)
                .collect();
            base + transaction_lanes_as_vecs.serialized_length()
        };
        base + self.preinstalls.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.enabled.write_bytes(writer)?;
        self.chain_id.write_bytes(writer)?;
        self.spec.write_bytes(writer)?;
        self.block_gas_limit.write_bytes(writer)?;
        self.base_fee.write_bytes(writer)?;
        self.wei_per_mote.write_bytes(writer)?;
        let transaction_lanes_as_vecs: Vec<Vec<u64>> = self
            .transaction_lanes
            .iter()
            .map(transaction_lane_definition_to_vec)
            .collect();
        transaction_lanes_as_vecs.write_bytes(writer)?;
        self.preinstalls.write_bytes(writer)
    }
}

impl FromBytes for EvmConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (enabled, remainder) = bool::from_bytes(bytes)?;
        let (chain_id, remainder) = u64::from_bytes(remainder)?;
        let (spec, remainder) = EvmSpec::from_bytes(remainder)?;
        let (block_gas_limit, remainder) = u64::from_bytes(remainder)?;
        let (base_fee, remainder) = u64::from_bytes(remainder)?;
        let (wei_per_mote, remainder) = u64::from_bytes(remainder)?;
        let (raw_transaction_lanes, remainder): (Vec<Vec<u64>>, &[u8]) =
            FromBytes::from_bytes(remainder)?;
        let transaction_lanes: Result<Vec<TransactionLaneDefinition>, _> = raw_transaction_lanes
            .into_iter()
            .map(TransactionLaneDefinition::try_from)
            .collect();
        let (preinstalls, remainder) =
            BTreeMap::<crate::evm::Address, Bytes>::from_bytes(remainder)?;
        if preinstalls.values().any(|code| code.is_empty()) {
            return Err(bytesrepr::Error::Formatting);
        }
        Ok((
            EvmConfig {
                enabled,
                chain_id,
                spec,
                block_gas_limit,
                base_fee,
                wei_per_mote,
                transaction_lanes: transaction_lanes.map_err(|_| bytesrepr::Error::Formatting)?,
                preinstalls,
            },
            remainder,
        ))
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::String;

    use serde_json::{json, Value};

    use super::*;

    fn config_json(preinstalls: BTreeMap<String, String>) -> Value {
        json!({
            "enabled": true,
            "chain_id": 7,
            "spec": "prague",
            "block_gas_limit": 30_000_000,
            "base_fee": 5_000,
            "wei_per_mote": 1_000_000_000,
            "preinstalls": preinstalls,
            "transaction_lanes": [],
        })
    }

    #[test]
    fn omitted_preinstalls_default_to_empty() {
        let mut value = serde_json::to_value(EvmConfig::default()).unwrap();
        value.as_object_mut().unwrap().remove("preinstalls");
        let config: EvmConfig = serde_json::from_value(value).unwrap();
        assert!(config.preinstalls.is_empty());
    }

    #[test]
    fn preinstalls_roundtrip_as_prefixed_base16_and_binary() {
        let value = config_json(BTreeMap::from([(
            "0xAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into(),
            "0x60AB00".into(),
        )]));
        let config: EvmConfig = serde_json::from_value(value).unwrap();
        let address = crate::evm::Address::new([0xaa; 20]);
        assert_eq!(config.preinstalls[&address].as_slice(), &[0x60, 0xab, 0]);
        let value = serde_json::to_value(&config).unwrap();
        assert_eq!(
            value["preinstalls"]["0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"],
            "0x60ab00"
        );
        assert_eq!(serde_json::from_value::<EvmConfig>(value).unwrap(), config);
        let serialized = config.to_bytes().unwrap();
        assert_eq!(serialized.len(), config.serialized_length());
        let (decoded, remainder) = EvmConfig::from_bytes(&serialized).unwrap();
        assert_eq!(decoded, config);
        assert!(remainder.is_empty());
    }

    #[test]
    fn preinstalls_reject_invalid_hex_and_empty_code() {
        for (address, code) in [
            ("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "0x00"),
            ("0x01", "0x00"),
            ("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "00"),
            ("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "0x0"),
            ("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "0xgg"),
            ("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "0x"),
        ] {
            let value = config_json(BTreeMap::from([(address.into(), code.into())]));
            assert!(serde_json::from_value::<EvmConfig>(value).is_err());
        }

        let mut value = config_json(BTreeMap::new());
        value["preinstalls"]["0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"] = json!([0]);
        assert!(serde_json::from_value::<EvmConfig>(value).is_err());
    }

    #[test]
    fn preinstalls_reject_duplicate_addresses_with_different_casing() {
        let value = config_json(BTreeMap::from([
            (
                "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
                "0x00".into(),
            ),
            (
                "0xAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into(),
                "0x01".into(),
            ),
        ]));
        let error = serde_json::from_value::<EvmConfig>(value).unwrap_err();
        assert!(error.to_string().contains("duplicate preinstall address"));
    }

    #[test]
    fn preinstalls_are_included_in_the_chainspec_hash() {
        let mut chainspec = crate::Chainspec::default();
        let empty_hash = chainspec.hash();
        let address = crate::evm::Address::new([1; 20]);
        chainspec
            .evm_config
            .preinstalls
            .insert(address, vec![0x00].into());
        let installed_hash = chainspec.hash();
        assert_ne!(empty_hash, installed_hash);
        chainspec
            .evm_config
            .preinstalls
            .insert(address, vec![0x01].into());
        assert_ne!(installed_hash, chainspec.hash());
        chainspec.evm_config.preinstalls.remove(&address);
        chainspec
            .evm_config
            .preinstalls
            .insert(crate::evm::Address::new([2; 20]), vec![0x00].into());
        assert_ne!(installed_hash, chainspec.hash());
    }

    #[test]
    fn preinstall_serialization_is_independent_of_input_order() {
        let first = config_json(BTreeMap::from([
            (
                "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
                "0x00".into(),
            ),
            (
                "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
                "0x01".into(),
            ),
        ]));
        let second = config_json(BTreeMap::from([
            (
                "0xBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB".into(),
                "0x01".into(),
            ),
            (
                "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
                "0x00".into(),
            ),
        ]));
        let first: EvmConfig = serde_json::from_value(first).unwrap();
        let second: EvmConfig = serde_json::from_value(second).unwrap();
        assert_eq!(first.to_bytes().unwrap(), second.to_bytes().unwrap());
    }

    #[test]
    fn binary_preinstalls_reject_empty_runtime_bytecode() {
        let mut config = EvmConfig::default();
        config
            .preinstalls
            .insert(crate::evm::Address::new([1; 20]), Bytes::new());
        assert!(EvmConfig::from_bytes(&config.to_bytes().unwrap()).is_err());
    }

    #[test]
    fn should_scale_base_fee_to_wei() {
        let config = EvmConfig {
            base_fee: 3,
            ..Default::default()
        };

        assert_eq!(config.base_fee_wei(), 3_000_000_000u128);
    }

    #[test]
    fn should_convert_wei_gas_fee_to_motes() {
        let config = EvmConfig::default();

        assert_eq!(
            config.gas_fee_motes(21_000, u128::from(DEFAULT_WEI_PER_MOTE)),
            Some(U512::from(21_000))
        );
    }

    #[test]
    fn should_round_sub_mote_wei_gas_fee_up_to_one_mote() {
        let config = EvmConfig::default();

        assert_eq!(config.gas_fee_motes(1, 1), Some(U512::from(1)));
    }

    #[test]
    fn should_convert_exact_evm_value_from_wei_to_motes() {
        let config = EvmConfig::default();

        assert_eq!(
            config.value_motes(U256::from(DEFAULT_WEI_PER_MOTE) * U256::from(15u64)),
            Some(U256::from(15u64))
        );
    }

    #[test]
    fn should_reject_evm_value_with_fractional_mote() {
        let config = EvmConfig::default();

        assert_eq!(config.value_motes(U256::from(1u64)), None);
    }

    fn config_with_lanes() -> EvmConfig {
        let mut config = EvmConfig::default();
        config.enabled = true;
        config.set_transaction_lanes(vec![
            TransactionLaneDefinition::new(100, 1_000, 100, 1_000_000, 5),
            TransactionLaneDefinition::new(101, 10_000, 1_000, 10_000_000, 2),
        ]);
        config
    }

    #[test]
    fn should_pick_smallest_fitting_lane() {
        let config = config_with_lanes();
        assert_eq!(config.get_evm_lane_id(500, 500, 50), Some(100));
    }

    #[test]
    fn should_take_gas_limit_into_account() {
        let config = config_with_lanes();
        // Small enough for lane 100 by size, but gas limit only fits lane 101.
        assert_eq!(config.get_evm_lane_id(2_000_000, 500, 50), Some(101));
    }

    #[test]
    fn should_take_calldata_size_into_account() {
        let config = config_with_lanes();
        // Small enough for lane 100 by size and gas, but calldata only fits lane 101.
        assert_eq!(config.get_evm_lane_id(500, 500, 500), Some(101));
    }

    #[test]
    fn should_return_none_when_no_lane_fits() {
        let config = config_with_lanes();
        assert_eq!(config.get_evm_lane_id(u64::MAX, 500, 50), None);
    }

    #[test]
    fn should_return_none_when_no_lanes_configured() {
        let config = EvmConfig::default();
        assert_eq!(config.get_evm_lane_id(1, 1, 1), None);
    }

    #[test]
    fn should_report_supported_lanes() {
        let config = config_with_lanes();
        assert!(config.is_supported(100));
        assert!(config.is_supported(101));
        assert!(!config.is_supported(102));
        let mut supported = config.get_supported_lanes();
        supported.sort_unstable();
        assert_eq!(supported, vec![100, 101]);
    }

    #[test]
    fn should_sum_max_evm_transaction_count() {
        let config = config_with_lanes();
        assert_eq!(config.get_max_evm_transaction_count(), Some(7));
    }

    #[test]
    fn should_return_none_max_evm_transaction_count_when_evm_disabled() {
        let mut config = config_with_lanes();
        config.enabled = false;
        assert_eq!(config.get_max_evm_transaction_count(), None);
    }

    #[test]
    fn should_bytesrepr_roundtrip_with_transaction_lanes() {
        let config = config_with_lanes();
        bytesrepr::test_serialization_roundtrip(&config);
    }

    #[test]
    fn should_serde_roundtrip_with_transaction_lanes() {
        let config = config_with_lanes();
        let serialized = serde_json::to_string(&config).expect("should serialize");
        let deserialized: EvmConfig =
            serde_json::from_str(&serialized).expect("should deserialize");
        assert_eq!(config, deserialized);
    }
}
