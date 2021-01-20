use std::{
    fmt::{self, Debug, Formatter},
    path::Path,
    str::FromStr,
};

use csv::ReaderBuilder;
use datasize::DataSize;
use num::rational::Ratio;
use num_traits::Zero;
#[cfg(test)]
use rand::Rng;
use semver::Version;
use serde::{Deserialize, Serialize};
use tracing::{error, warn};

use casper_execution_engine::{
    core::engine_state::genesis::{ExecConfig, GenesisAccount},
    shared::{motes::Motes, system_config::SystemConfig, wasm_config::WasmConfig},
};
use casper_types::{auction::EraId, PublicKey, U512};

use super::{config, error::GenesisLoadError, Error};
#[cfg(test)]
use crate::testing::TestRng;
use crate::{
    crypto::hash::{self, Digest},
    types::{TimeDiff, Timestamp},
    utils::Loadable,
};

#[derive(Copy, Clone, DataSize, Debug, PartialEq, Eq, Serialize, Deserialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct DeployConfig {
    pub(crate) max_payment_cost: Motes,
    pub(crate) max_ttl: TimeDiff,
    pub(crate) max_dependencies: u8,
    pub(crate) max_block_size: u32,
    pub(crate) block_max_deploy_count: u32,
    pub(crate) block_max_transfer_count: u32,
    pub(crate) block_gas_limit: u64,
}

impl Default for DeployConfig {
    fn default() -> Self {
        DeployConfig {
            max_payment_cost: Motes::zero(),
            max_ttl: TimeDiff::from_str("1day").unwrap(),
            max_dependencies: 10,
            max_block_size: 10_485_760,
            block_max_deploy_count: 10,
            block_max_transfer_count: 1000,
            block_gas_limit: 10_000_000_000_000,
        }
    }
}

#[cfg(test)]
impl DeployConfig {
    /// Generates a random instance using a `TestRng`.
    pub fn random(rng: &mut TestRng) -> Self {
        let max_payment_cost = Motes::new(U512::from(
            rng.gen_range::<_, u64, u64>(1_000_000, 1_000_000_000),
        ));
        let max_ttl = TimeDiff::from(rng.gen_range(60_000, 3_600_000));
        let max_dependencies = rng.gen();
        let max_block_size = rng.gen_range(1_000_000, 1_000_000_000);
        let block_max_deploy_count = rng.gen();
        let block_max_transfer_count = rng.gen();
        let block_gas_limit = rng.gen_range(100_000_000_000, 1_000_000_000_000_000);

        DeployConfig {
            max_payment_cost,
            max_ttl,
            max_dependencies,
            max_block_size,
            block_max_deploy_count,
            block_max_transfer_count,
            block_gas_limit,
        }
    }
}

#[derive(Clone, DataSize, Debug, PartialEq, Eq, Serialize, Deserialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub(crate) struct HighwayConfig {
    // TODO: Most of these are not Highway-specific.
    // TODO: Some should be defined on-chain in a contract instead, or be part of `UpgradePoint`.
    pub(crate) era_duration: TimeDiff,
    pub(crate) minimum_era_height: u64,
    #[data_size(skip)]
    pub(crate) finality_threshold_fraction: Ratio<u64>,
    pub(crate) minimum_round_exponent: u8,
    pub(crate) maximum_round_exponent: u8,
    /// The factor by which rewards for a round are multiplied if the greatest summit has ≤50%
    /// quorum, i.e. no finality.
    #[data_size(skip)]
    pub(crate) reduced_reward_multiplier: Ratio<u64>,
}

impl Default for HighwayConfig {
    fn default() -> Self {
        HighwayConfig {
            era_duration: TimeDiff::from_str("1week").unwrap(),
            minimum_era_height: 100,
            finality_threshold_fraction: Ratio::new(1, 3),
            minimum_round_exponent: 14, // 2**14 ms = ~16 seconds
            maximum_round_exponent: 19, // 2**19 ms = ~8.7 minutes
            reduced_reward_multiplier: Ratio::new(1, 5),
        }
    }
}

impl HighwayConfig {
    /// Checks whether the values set in the config make sense and prints warnings if they don't
    pub fn validate_config(&self) {
        let min_era_ms = 1u64 << self.minimum_round_exponent;
        // if the era duration is set to zero, we will treat it as explicitly stating that eras
        // should be defined by height only
        if self.era_duration.millis() > 0
            && self.era_duration.millis() < self.minimum_era_height * min_era_ms
        {
            warn!("Era duration is less than minimum era height * round length!");
        }

        if self.minimum_round_exponent > self.maximum_round_exponent {
            panic!(
                "Minimum round exponent is greater than the maximum round exponent.\n\
                 Minimum round exponent: {min},\n\
                 Maximum round exponent: {max}",
                min = self.minimum_round_exponent,
                max = self.maximum_round_exponent
            );
        }

        if self.finality_threshold_fraction <= Ratio::new(0, 1)
            || self.finality_threshold_fraction >= Ratio::new(1, 1)
        {
            panic!(
                "Finality threshold fraction is not in the range (0, 1)! Finality threshold: {ftt}",
                ftt = self.finality_threshold_fraction
            );
        }

        if self.reduced_reward_multiplier > Ratio::new(1, 1) {
            panic!(
                "Reduced reward multiplier is not in the range [0, 1]! Multiplier: {rrm}",
                rrm = self.reduced_reward_multiplier
            );
        }
    }
}

#[cfg(test)]
impl HighwayConfig {
    /// Generates a random instance using a `TestRng`.
    pub fn random(rng: &mut TestRng) -> Self {
        HighwayConfig {
            era_duration: TimeDiff::from(rng.gen_range(600_000, 604_800_000)),
            minimum_era_height: rng.gen_range(5, 100),
            finality_threshold_fraction: Ratio::new(rng.gen_range(1, 100), 100),
            minimum_round_exponent: rng.gen_range(0, 16),
            maximum_round_exponent: rng.gen_range(16, 22),
            reduced_reward_multiplier: Ratio::new(rng.gen_range(0, 10), 10),
        }
    }
}

impl Loadable for Vec<GenesisAccount> {
    type Error = GenesisLoadError;

    fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, Self::Error> {
        #[derive(Debug, Deserialize)]
        struct ParsedAccount {
            public_key: PublicKey,
            balance: U512,
            bonded_amount: U512,
        }

        let mut reader = ReaderBuilder::new().has_headers(false).from_path(path)?;
        let mut accounts = vec![];
        for result in reader.deserialize() {
            let parsed: ParsedAccount = result?;
            let balance = Motes::new(parsed.balance);
            let bonded_amount = Motes::new(parsed.bonded_amount);

            let account = GenesisAccount::new(
                parsed.public_key,
                parsed.public_key.to_account_hash(),
                balance,
                bonded_amount,
            );
            accounts.push(account);
        }
        Ok(accounts)
    }
}

#[derive(Clone, DataSize, PartialEq, Eq, Serialize, Deserialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct GenesisConfig {
    pub(crate) name: String,
    pub(crate) timestamp: Timestamp,
    pub(crate) validator_slots: u32,
    /// Number of eras before an auction actually defines the set of validators.
    /// If you bond with a sufficient bid in era N, you will be a validator in era N +
    /// auction_delay + 1
    pub(crate) auction_delay: u64,
    /// The delay for the payout of funds, in eras. If a withdraw request is included in a block in
    /// era N (other than the last one), they are paid out in the last block of era N +
    /// locked_funds_period.
    pub(crate) locked_funds_period: EraId,
    /// Round seigniorage rate represented as a fractional number.
    #[data_size(skip)]
    pub(crate) round_seigniorage_rate: Ratio<u64>,
    /// The delay for paying out the the unbonding amount.
    pub(crate) unbonding_delay: EraId,
    // We don't have an implementation for the semver version type, we skip it for now
    #[data_size(skip)]
    pub(crate) protocol_version: Version,
    pub(crate) accounts: Vec<GenesisAccount>,
    pub(crate) wasm_config: WasmConfig,
    pub(crate) system_config: SystemConfig,
    pub(crate) deploy_config: DeployConfig,
    pub(crate) highway_config: HighwayConfig,
}

impl GenesisConfig {
    /// Returns a vector of Genesis validators' public key and their stake.
    pub fn genesis_validator_stakes(&self) -> Vec<(PublicKey, Motes)> {
        self.accounts
            .iter()
            .filter_map(|genesis_account| {
                if genesis_account.is_genesis_validator() {
                    let public_key = genesis_account
                        .public_key()
                        .expect("should have genesis public key");

                    Some((public_key, genesis_account.bonded_amount()))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Checks whether the values set in the config make sense and prints warnings if they don't
    pub fn validate_config(&self) {
        self.highway_config.validate_config();
    }
}

impl Debug for GenesisConfig {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        formatter
            .debug_struct("GenesisConfig")
            .field("name", &self.name)
            .field("timestamp", &self.timestamp)
            .field(
                "protocol_version",
                &format_args!("{}", self.protocol_version),
            )
            .field("accounts", &self.accounts)
            .field("costs", &self.wasm_config)
            .field("deploy_config", &self.deploy_config)
            .field("highway_config", &self.highway_config)
            .finish()
    }
}

#[cfg(test)]
impl GenesisConfig {
    /// Generates a random instance using a `TestRng`.
    pub fn random(rng: &mut TestRng) -> Self {
        let name = rng.gen::<char>().to_string();
        let timestamp = Timestamp::random(rng);
        let validator_slots = rng.gen::<u32>();
        let auction_delay = rng.gen::<u64>();
        let locked_funds_period: EraId = rng.gen::<u64>();
        let round_seigniorage_rate = Ratio::new(
            rng.gen_range(1, 1_000_000_000),
            rng.gen_range(1, 1_000_000_000),
        );
        let protocol_version = Version::new(
            rng.gen_range(0, 10),
            rng.gen::<u8>() as u64,
            rng.gen::<u8>() as u64,
        );
        let accounts = vec![rng.gen(), rng.gen(), rng.gen(), rng.gen(), rng.gen()];
        let wasm_config = rng.gen();
        let deploy_config = DeployConfig::random(rng);
        let highway_config = HighwayConfig::random(rng);
        let unbonding_delay = rng.gen();
        let system_config = rng.gen();

        GenesisConfig {
            name,
            timestamp,
            validator_slots,
            auction_delay,
            locked_funds_period,
            round_seigniorage_rate,
            unbonding_delay,
            protocol_version,
            accounts,
            wasm_config,
            system_config,
            deploy_config,
            highway_config,
        }
    }
}

#[derive(Copy, Clone, DataSize, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ActivationPoint {
    pub(crate) height: u64,
}

#[derive(Clone, DataSize, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct UpgradePoint {
    pub(crate) activation_point: ActivationPoint,
    #[data_size(skip)]
    pub(crate) protocol_version: Version,
    pub(crate) new_wasm_config: Option<WasmConfig>,
    pub(crate) new_system_config: Option<SystemConfig>,
    pub(crate) new_deploy_config: Option<DeployConfig>,
    pub(crate) new_validator_slots: Option<u32>,
}

#[cfg(test)]
impl UpgradePoint {
    /// Generates a random instance using a `TestRng`.
    pub fn random(rng: &mut TestRng) -> Self {
        let activation_point = ActivationPoint {
            height: rng.gen::<u8>() as u64,
        };
        let protocol_version = Version::new(
            rng.gen_range(10, 20),
            rng.gen::<u8>() as u64,
            rng.gen::<u8>() as u64,
        );
        let new_costs = if rng.gen() { Some(rng.gen()) } else { None };
        let new_deploy_config = if rng.gen() {
            Some(DeployConfig::random(rng))
        } else {
            None
        };
        let new_validator_slots = rng.gen::<Option<u32>>();
        let new_system_config = rng.gen();

        UpgradePoint {
            activation_point,
            protocol_version,
            new_wasm_config: new_costs,
            new_system_config,
            new_deploy_config,
            new_validator_slots,
        }
    }
}

/// A collection of configuration settings describing the state of the system at genesis and
/// upgrades to basic system functionality (including system contracts and gas costs) occurring
/// after genesis.
#[derive(Clone, DataSize, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chainspec {
    pub(crate) genesis: GenesisConfig,
    pub(crate) upgrades: Vec<UpgradePoint>,
}

impl Loadable for Chainspec {
    type Error = Error;
    fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, Self::Error> {
        config::parse_toml(path)
    }
}

impl Chainspec {
    /// Checks whether the values set in the config make sense and prints warnings if they don't
    pub fn validate_config(&self) {
        self.genesis.validate_config();
    }

    /// Serializes `self` and hashes the resulting bytes.
    pub(crate) fn hash(&self) -> Digest {
        let serialized_chainspec = bincode::serialize(self).unwrap_or_else(|error| {
            error!("failed to serialize chainspec: {}", error);
            vec![]
        });
        hash::hash(&serialized_chainspec)
    }
}

#[cfg(test)]
impl Chainspec {
    /// Generates a random instance using a `TestRng`.
    pub fn random(rng: &mut TestRng) -> Self {
        let genesis = GenesisConfig::random(rng);
        let upgrades = vec![UpgradePoint::random(rng), UpgradePoint::random(rng)];
        Chainspec { genesis, upgrades }
    }
}

impl Into<ExecConfig> for Chainspec {
    fn into(self) -> ExecConfig {
        ExecConfig::new(
            self.genesis.accounts,
            self.genesis.wasm_config,
            self.genesis.system_config,
            self.genesis.validator_slots,
            self.genesis.auction_delay,
            self.genesis.locked_funds_period,
            self.genesis.round_seigniorage_rate,
            self.genesis.unbonding_delay,
        )
    }
}

#[cfg(test)]
mod tests {
    use once_cell::sync::Lazy;

    use casper_execution_engine::shared::{
        host_function_costs::{HostFunction, HostFunctionCosts},
        opcode_costs::OpcodeCosts,
        storage_costs::StorageCosts,
        wasm_config::WasmConfig,
    };

    static EXPECTED_GENESIS_HOST_FUNCTION_COSTS: Lazy<HostFunctionCosts> =
        Lazy::new(|| HostFunctionCosts {
            read_value: HostFunction::new(127, [0, 1, 0]),
            read_value_local: HostFunction::new(128, [0, 1, 0]),
            write: HostFunction::new(140, [0, 1, 0, 2]),
            write_local: HostFunction::new(141, [0, 1, 2, 3]),
            add: HostFunction::new(100, [0, 1, 2, 3]),
            new_uref: HostFunction::new(122, [0, 1, 2]),
            load_named_keys: HostFunction::new(121, [0, 1]),
            ret: HostFunction::new(133, [0, 1]),
            get_key: HostFunction::new(113, [0, 1, 2, 3, 4]),
            has_key: HostFunction::new(119, [0, 1]),
            put_key: HostFunction::new(125, [0, 1, 2, 3]),
            remove_key: HostFunction::new(132, [0, 1]),
            revert: HostFunction::new(134, [0]),
            is_valid_uref: HostFunction::new(120, [0, 1]),
            add_associated_key: HostFunction::new(101, [0, 1, 2]),
            remove_associated_key: HostFunction::new(129, [0, 1]),
            update_associated_key: HostFunction::new(139, [0, 1, 2]),
            set_action_threshold: HostFunction::new(135, [0, 1]),
            get_caller: HostFunction::new(112, [0]),
            get_blocktime: HostFunction::new(111, [0]),
            create_purse: HostFunction::new(108, [0, 1]),
            transfer_to_account: HostFunction::new(138, [0, 1, 2, 3, 4, 5, 6]),
            transfer_from_purse_to_account: HostFunction::new(136, [0, 1, 2, 3, 4, 5, 6, 7, 8]),
            transfer_from_purse_to_purse: HostFunction::new(137, [0, 1, 2, 3, 4, 5, 6, 7]),
            get_balance: HostFunction::new(110, [0, 1, 2]),
            get_phase: HostFunction::new(117, [0]),
            get_system_contract: HostFunction::new(118, [0, 1, 2]),
            get_main_purse: HostFunction::new(114, [0]),
            read_host_buffer: HostFunction::new(126, [0, 1, 2]),
            create_contract_package_at_hash: HostFunction::new(106, [0, 1]),
            create_contract_user_group: HostFunction::new(107, [0, 1, 2, 3, 4, 5, 6, 7]),
            add_contract_version: HostFunction::new(102, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]),
            disable_contract_version: HostFunction::new(109, [0, 1, 2, 3]),
            call_contract: HostFunction::new(104, [0, 1, 2, 3, 4, 5, 6]),
            call_versioned_contract: HostFunction::new(105, [0, 1, 2, 3, 4, 5, 6, 7, 8]),
            get_named_arg_size: HostFunction::new(116, [0, 1, 2]),
            get_named_arg: HostFunction::new(115, [0, 1, 2, 3]),
            remove_contract_user_group: HostFunction::new(130, [0, 1, 2, 3]),
            provision_contract_user_group_uref: HostFunction::new(124, [0, 1, 2, 3, 4]),
            remove_contract_user_group_urefs: HostFunction::new(131, [0, 1, 2, 3, 4, 5]),
            print: HostFunction::new(123, [0, 1]),
            blake2b: HostFunction::new(133, [0, 1, 2, 3]),
        });
    static EXPECTED_GENESIS_WASM_CONFIG: Lazy<WasmConfig> = Lazy::new(|| {
        WasmConfig::new(
            17, // initial_memory
            19, // max_stack_height
            EXPECTED_GENESIS_COSTS,
            EXPECTED_GENESIS_STORAGE_COSTS,
            *EXPECTED_GENESIS_HOST_FUNCTION_COSTS,
        )
    });

    const EXPECTED_GENESIS_STORAGE_COSTS: StorageCosts = StorageCosts::new(101);

    const EXPECTED_GENESIS_COSTS: OpcodeCosts = OpcodeCosts {
        bit: 13,
        add: 14,
        mul: 15,
        div: 16,
        load: 17,
        store: 18,
        op_const: 19,
        local: 20,
        global: 21,
        control_flow: 22,
        integer_comparsion: 23,
        conversion: 24,
        unreachable: 25,
        nop: 26,
        current_memory: 27,
        grow_memory: 28,
        regular: 29,
    };
    const EXPECTED_UPGRADE_COSTS: OpcodeCosts = OpcodeCosts {
        bit: 24,
        add: 25,
        mul: 26,
        div: 27,
        load: 28,
        store: 29,
        op_const: 30,
        local: 31,
        global: 32,
        control_flow: 33,
        integer_comparsion: 34,
        conversion: 35,
        unreachable: 36,
        nop: 37,
        current_memory: 38,
        grow_memory: 39,
        regular: 40,
    };

    use super::*;
    use crate::testing;

    fn check_spec(spec: Chainspec) {
        assert_eq!(spec.genesis.name, "test-chain");
        assert_eq!(spec.genesis.timestamp.millis(), 1600454700000);
        assert_eq!(spec.genesis.protocol_version, Version::from((0, 1, 0)));

        assert_eq!(spec.genesis.accounts.len(), 4);
        for index in 0..4 {
            assert_eq!(
                spec.genesis.accounts[index].balance(),
                Motes::new(U512::from(index + 1))
            );
            assert_eq!(
                spec.genesis.accounts[index].bonded_amount(),
                Motes::new(U512::from((index as u64 + 1) * 10))
            );
        }

        assert_eq!(
            spec.genesis.highway_config.era_duration,
            TimeDiff::from(180000)
        );
        assert_eq!(spec.genesis.highway_config.minimum_era_height, 9);
        assert_eq!(
            spec.genesis.highway_config.finality_threshold_fraction,
            Ratio::new(2, 25)
        );
        assert_eq!(spec.genesis.highway_config.minimum_round_exponent, 14);
        assert_eq!(spec.genesis.highway_config.maximum_round_exponent, 19);
        assert_eq!(
            spec.genesis.highway_config.reduced_reward_multiplier,
            Ratio::new(1, 5)
        );

        assert_eq!(
            spec.genesis.deploy_config.max_payment_cost,
            Motes::new(U512::from(9))
        );
        assert_eq!(
            spec.genesis.deploy_config.max_ttl,
            TimeDiff::from(26300160000)
        );
        assert_eq!(spec.genesis.deploy_config.max_dependencies, 11);
        assert_eq!(spec.genesis.deploy_config.max_block_size, 12);
        assert_eq!(spec.genesis.deploy_config.block_max_deploy_count, 125);
        assert_eq!(spec.genesis.deploy_config.block_gas_limit, 13);

        assert_eq!(spec.genesis.wasm_config, *EXPECTED_GENESIS_WASM_CONFIG);

        assert_eq!(spec.upgrades.len(), 2);

        let upgrade0 = &spec.upgrades[0];
        assert_eq!(upgrade0.activation_point, ActivationPoint { height: 23 });
        assert_eq!(upgrade0.protocol_version, Version::from((0, 2, 0)));

        let new_wasm_config = upgrade0
            .new_wasm_config
            .as_ref()
            .expect("should have new wasm config");

        assert_eq!(
            upgrade0.new_wasm_config.as_ref().unwrap().opcode_costs(),
            EXPECTED_UPGRADE_COSTS,
        );

        assert_eq!(new_wasm_config.max_memory, 17);
        assert_eq!(new_wasm_config.max_stack_height, 19);

        assert_eq!(
            upgrade0.new_deploy_config.unwrap().max_payment_cost,
            Motes::new(U512::from(34))
        );
        assert_eq!(
            upgrade0.new_deploy_config.unwrap().max_ttl,
            TimeDiff::from(1104516000000)
        );
        assert_eq!(upgrade0.new_deploy_config.unwrap().max_dependencies, 36);
        assert_eq!(upgrade0.new_deploy_config.unwrap().max_block_size, 37);
        assert_eq!(
            upgrade0.new_deploy_config.unwrap().block_max_deploy_count,
            375
        );
        assert_eq!(
            upgrade0.new_deploy_config.unwrap().block_max_transfer_count,
            376
        );
        assert_eq!(upgrade0.new_deploy_config.unwrap().block_gas_limit, 38);

        let upgrade1 = &spec.upgrades[1];
        assert_eq!(upgrade1.activation_point, ActivationPoint { height: 39 });
        assert_eq!(upgrade1.protocol_version, Version::from((0, 3, 0)));
        assert!(upgrade1.new_wasm_config.is_none());
        assert!(upgrade1.new_deploy_config.is_none());
    }

    #[test]
    fn check_bundled_spec() {
        let spec = Chainspec::from_resources("test/valid/chainspec.toml");
        check_spec(spec);
    }

    #[test]
    fn bincode_roundtrip() {
        let mut rng = crate::new_rng();
        let chainspec = Chainspec::random(&mut rng);
        testing::bincode_roundtrip(&chainspec);
    }
}
