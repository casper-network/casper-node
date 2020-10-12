use std::{
    convert::TryInto,
    fmt::{self, Debug, Formatter},
    path::Path,
    str::FromStr,
};

use csv::ReaderBuilder;
use datasize::DataSize;
use num_traits::Zero;
#[cfg(test)]
use rand::Rng;
use semver::Version;
use serde::{Deserialize, Serialize};

use casper_execution_engine::{
    core::engine_state::genesis::{ExecConfig, GenesisAccount},
    shared::{motes::Motes, wasm_config::WasmConfig},
};
use casper_types::U512;

use super::{config, error::GenesisLoadError, Error};
#[cfg(test)]
use crate::testing::TestRng;
use crate::{
    crypto::asymmetric_key::PublicKey,
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
        let block_gas_limit = rng.gen_range(100_000_000_000, 1_000_000_000_000_000);

        DeployConfig {
            max_payment_cost,
            max_ttl,
            max_dependencies,
            max_block_size,
            block_max_deploy_count,
            block_gas_limit,
        }
    }
}

#[derive(Copy, Clone, DataSize, Debug, PartialEq, Eq, Serialize, Deserialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub(crate) struct HighwayConfig {
    // TODO: Most of these are not Highway-specific.
    // TODO: Some should be defined on-chain in a contract instead, or be part of `UpgradePoint`.
    pub(crate) genesis_era_start_timestamp: Timestamp,
    pub(crate) era_duration: TimeDiff,
    pub(crate) minimum_era_height: u64,
    // TODO: This is duplicated (and probably in conflict with) `AUCTION_DELAY`.
    pub(crate) booking_duration: TimeDiff,
    pub(crate) entropy_duration: TimeDiff,
    // TODO: Do we need this? When we see the switch block finalized it should suffice to keep
    // gossiping, without producing new votes. Everyone else will eventually see the same finality.
    pub(crate) voting_period_duration: TimeDiff,
    pub(crate) finality_threshold_percent: u8,
    pub(crate) minimum_round_exponent: u8,
}

impl Default for HighwayConfig {
    fn default() -> Self {
        HighwayConfig {
            genesis_era_start_timestamp: Timestamp::from_str("2020-10-01T00:00:01.000Z").unwrap(),
            era_duration: TimeDiff::from_str("1week").unwrap(),
            minimum_era_height: 100,
            booking_duration: TimeDiff::from_str("10days").unwrap(),
            entropy_duration: TimeDiff::from_str("3hours").unwrap(),
            voting_period_duration: TimeDiff::from_str("2days").unwrap(),
            finality_threshold_percent: 10,
            minimum_round_exponent: 14, // 2**14 ms = ~16 seconds
        }
    }
}

#[cfg(test)]
impl HighwayConfig {
    /// Generates a random instance using a `TestRng`.
    pub fn random(rng: &mut TestRng) -> Self {
        HighwayConfig {
            genesis_era_start_timestamp: Timestamp::random(rng),
            era_duration: TimeDiff::from(rng.gen_range(600_000, 604_800_000)),
            minimum_era_height: rng.gen_range(5, 100),
            booking_duration: TimeDiff::from(rng.gen_range(600_000, 864_000_000)),
            entropy_duration: TimeDiff::from(rng.gen_range(600_000, 10_800_000)),
            voting_period_duration: TimeDiff::from(rng.gen_range(600_000, 172_800_000)),
            finality_threshold_percent: rng.gen_range(0, 101),
            minimum_round_exponent: rng.gen_range(0, 20),
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
                casper_types::PublicKey::from(parsed.public_key),
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
    // We don't have an implementation for the semver version type, we skip it for now
    #[data_size(skip)]
    pub(crate) protocol_version: Version,
    pub(crate) mint_installer_bytes: Vec<u8>,
    pub(crate) pos_installer_bytes: Vec<u8>,
    pub(crate) standard_payment_installer_bytes: Vec<u8>,
    pub(crate) auction_installer_bytes: Vec<u8>,
    pub(crate) accounts: Vec<GenesisAccount>,
    pub(crate) wasm_config: WasmConfig,
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

                    let crypto_public_key = public_key
                        .try_into()
                        .expect("should have valid genesis public key");

                    Some((crypto_public_key, genesis_account.bonded_amount()))
                } else {
                    None
                }
            })
            .clone()
            .collect()
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
            .field(
                "mint_installer_bytes",
                &format_args!("[{} bytes]", self.mint_installer_bytes.len()),
            )
            .field(
                "pos_installer_bytes",
                &format_args!("[{} bytes]", self.pos_installer_bytes.len()),
            )
            .field(
                "standard_payment_installer_bytes",
                &format_args!("[{} bytes]", self.standard_payment_installer_bytes.len()),
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
        let protocol_version = Version::new(
            rng.gen_range(0, 10),
            rng.gen::<u8>() as u64,
            rng.gen::<u8>() as u64,
        );
        let mint_installer_bytes = vec![rng.gen()];
        let pos_installer_bytes = vec![rng.gen()];
        let standard_payment_installer_bytes = vec![rng.gen()];
        let auction_installer_bytes = vec![rng.gen()];
        let accounts = vec![rng.gen(), rng.gen(), rng.gen(), rng.gen(), rng.gen()];
        let costs = rng.gen();
        let deploy_config = DeployConfig::random(rng);
        let highway_config = HighwayConfig::random(rng);

        GenesisConfig {
            name,
            timestamp,
            protocol_version,
            mint_installer_bytes,
            pos_installer_bytes,
            standard_payment_installer_bytes,
            auction_installer_bytes,
            accounts,
            wasm_config: costs,
            deploy_config,
            highway_config,
        }
    }
}

#[derive(Copy, Clone, DataSize, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ActivationPoint {
    pub(crate) rank: u64,
}

#[derive(Clone, DataSize, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct UpgradePoint {
    pub(crate) activation_point: ActivationPoint,
    #[data_size(skip)]
    pub(crate) protocol_version: Version,
    pub(crate) upgrade_installer_bytes: Option<Vec<u8>>,
    pub(crate) upgrade_installer_args: Option<Vec<u8>>,
    pub(crate) new_wasm_config: Option<WasmConfig>,
    pub(crate) new_deploy_config: Option<DeployConfig>,
}

#[cfg(test)]
impl UpgradePoint {
    /// Generates a random instance using a `TestRng`.
    pub fn random(rng: &mut TestRng) -> Self {
        let activation_point = ActivationPoint {
            rank: rng.gen::<u8>() as u64,
        };
        let protocol_version = Version::new(
            rng.gen_range(10, 20),
            rng.gen::<u8>() as u64,
            rng.gen::<u8>() as u64,
        );
        let upgrade_installer_bytes = if rng.gen() {
            Some(vec![rng.gen()])
        } else {
            None
        };
        let upgrade_installer_args = if rng.gen() {
            Some(vec![rng.gen()])
        } else {
            None
        };
        let new_costs = if rng.gen() { Some(rng.gen()) } else { None };
        let new_deploy_config = if rng.gen() {
            Some(DeployConfig::random(rng))
        } else {
            None
        };

        UpgradePoint {
            activation_point,
            protocol_version,
            upgrade_installer_bytes,
            upgrade_installer_args,
            new_wasm_config: new_costs,
            new_deploy_config,
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
            self.genesis.mint_installer_bytes,
            self.genesis.pos_installer_bytes,
            self.genesis.standard_payment_installer_bytes,
            self.genesis.auction_installer_bytes,
            self.genesis.accounts,
            self.genesis.wasm_config,
        )
    }
}

#[cfg(test)]
mod tests {
    use casper_execution_engine::shared::{
        host_function_costs::{HostFunction, HostFunctionCosts},
        opcode_costs::OpcodeCosts,
        storage_costs::StorageCosts,
        wasm_config::WasmConfig,
    };

    const EXPECTED_GENESIS_HOST_FUNCTION_COSTS: HostFunctionCosts = HostFunctionCosts {
        read_value: HostFunction {
            cost: 127,
            arguments: (0, 1, 0),
        },
        read_value_local: HostFunction {
            cost: 128,
            arguments: (0, 1, 0),
        },
        write: HostFunction {
            cost: 140,
            arguments: (0, 1, 0, 2),
        },
        write_local: HostFunction {
            cost: 141,
            arguments: (0, 1, 2, 3),
        },
        add: HostFunction {
            cost: 100,
            arguments: (0, 1, 2, 3),
        },
        add_local: HostFunction {
            cost: 103,
            arguments: (0, 1, 2, 3),
        },
        new_uref: HostFunction {
            cost: 122,
            arguments: (0, 1, 2),
        },
        load_named_keys: HostFunction {
            cost: 121,
            arguments: (0, 1),
        },
        ret: HostFunction {
            cost: 133,
            arguments: (0, 1),
        },
        get_key: HostFunction {
            cost: 113,
            arguments: (0, 1, 2, 3, 4),
        },
        has_key: HostFunction {
            cost: 119,
            arguments: (0, 1),
        },
        put_key: HostFunction {
            cost: 125,
            arguments: (0, 1),
        },
        remove_key: HostFunction {
            cost: 132,
            arguments: (0, 1),
        },
        revert: HostFunction {
            cost: 134,
            arguments: (0,),
        },
        is_valid_uref: HostFunction {
            cost: 120,
            arguments: (0, 1),
        },
        add_associated_key: HostFunction {
            cost: 101,
            arguments: (0, 1, 2, 3),
        },
        remove_associated_key: HostFunction {
            cost: 129,
            arguments: (0, 1),
        },
        update_associated_key: HostFunction {
            cost: 139,
            arguments: (0, 1, 2),
        },
        set_action_threshold: HostFunction {
            cost: 135,
            arguments: (0, 1),
        },
        get_caller: HostFunction {
            cost: 112,
            arguments: (0,),
        },
        get_blocktime: HostFunction {
            cost: 111,
            arguments: (0,),
        },
        create_purse: HostFunction {
            cost: 108,
            arguments: (0, 1),
        },
        transfer_to_account: HostFunction {
            cost: 138,
            arguments: (0, 1, 2, 3),
        },
        transfer_from_purse_to_account: HostFunction {
            cost: 136,
            arguments: (0, 1, 2, 3, 4, 5),
        },
        transfer_from_purse_to_purse: HostFunction {
            cost: 137,
            arguments: (0, 1, 2, 3, 4, 5),
        },
        get_balance: HostFunction {
            cost: 110,
            arguments: (0, 1, 2),
        },
        get_phase: HostFunction {
            cost: 117,
            arguments: (0,),
        },
        get_system_contract: HostFunction {
            cost: 118,
            arguments: (0, 1, 2),
        },
        get_main_purse: HostFunction {
            cost: 114,
            arguments: (0,),
        },
        read_host_buffer: HostFunction {
            cost: 126,
            arguments: (0, 1, 2),
        },
        create_contract_package_at_hash: HostFunction {
            cost: 106,
            arguments: (0, 1),
        },
        create_contract_user_group: HostFunction {
            cost: 107,
            arguments: (0, 1, 2, 3, 4, 5, 6, 7),
        },
        add_contract_version: HostFunction {
            cost: 102,
            arguments: (0, 1, 2, 3, 4, 5, 6, 7, 8, 9),
        },
        disable_contract_version: HostFunction {
            cost: 109,
            arguments: (0, 1, 2, 3),
        },
        call_contract: HostFunction {
            cost: 104,
            arguments: (0, 1, 2, 3, 4, 5, 6),
        },
        call_versioned_contract: HostFunction {
            cost: 105,
            arguments: (0, 1, 2, 3, 4, 5, 6, 7, 8),
        },
        get_named_arg_size: HostFunction {
            cost: 116,
            arguments: (0, 1, 2),
        },
        get_named_arg: HostFunction {
            cost: 115,
            arguments: (0, 1, 2, 3),
        },
        remove_contract_user_group: HostFunction {
            cost: 130,
            arguments: (0, 1, 2, 3),
        },
        provision_contract_user_group_uref: HostFunction {
            cost: 124,
            arguments: (0, 1, 2, 3, 4),
        },
        remove_contract_user_group_urefs: HostFunction {
            cost: 131,
            arguments: (0, 1, 2, 3, 4, 5),
        },
        print: HostFunction {
            cost: 123,
            arguments: (0, 1),
        },
    };
    const EXPECTED_GENESIS_WASM_CONFIG: WasmConfig = WasmConfig::new(
        17, // initial_memory
        19, // max_stack_height
        EXPECTED_GENESIS_COSTS,
        EXPECTED_GENESIS_STORAGE_COSTS,
        EXPECTED_GENESIS_HOST_FUNCTION_COSTS,
    );
    const EXPECTED_GENESIS_STORAGE_COSTS: StorageCosts = StorageCosts { gas_per_byte: 101 };

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
    use crate::testing::{self, TestRng};

    fn check_spec(spec: Chainspec) {
        assert_eq!(spec.genesis.name, "test-chain");
        assert_eq!(spec.genesis.timestamp.millis(), 1600454700000);
        assert_eq!(spec.genesis.protocol_version, Version::from((0, 1, 0)));
        assert_eq!(spec.genesis.mint_installer_bytes, b"Mint installer bytes");
        assert_eq!(
            spec.genesis.pos_installer_bytes,
            b"Proof of Stake installer bytes"
        );
        assert_eq!(
            spec.genesis.standard_payment_installer_bytes,
            b"Standard Payment installer bytes"
        );

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
            spec.genesis
                .highway_config
                .genesis_era_start_timestamp
                .millis(),
            1600454700000
        );
        assert_eq!(
            spec.genesis.highway_config.era_duration,
            TimeDiff::from(180000)
        );
        assert_eq!(spec.genesis.highway_config.minimum_era_height, 9);
        assert_eq!(
            spec.genesis.highway_config.booking_duration,
            TimeDiff::from(14400000)
        );
        assert_eq!(
            spec.genesis.highway_config.entropy_duration,
            TimeDiff::from(432000000)
        );
        assert_eq!(
            spec.genesis.highway_config.voting_period_duration,
            TimeDiff::from(3628800000)
        );
        assert_eq!(spec.genesis.highway_config.finality_threshold_percent, 8);
        assert_eq!(spec.genesis.highway_config.minimum_round_exponent, 13);

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

        assert_eq!(spec.genesis.wasm_config, EXPECTED_GENESIS_WASM_CONFIG);

        assert_eq!(spec.upgrades.len(), 2);

        let upgrade0 = &spec.upgrades[0];
        assert_eq!(upgrade0.activation_point, ActivationPoint { rank: 23 });
        assert_eq!(upgrade0.protocol_version, Version::from((0, 2, 0)));
        assert_eq!(
            upgrade0.upgrade_installer_bytes,
            Some(b"Upgrade installer bytes".to_vec())
        );
        assert!(upgrade0.upgrade_installer_args.is_none());

        let new_wasm_config = upgrade0
            .new_wasm_config
            .as_ref()
            .expect("should have new wasm config");

        assert_eq!(
            upgrade0.new_wasm_config.as_ref().unwrap().opcode_costs(),
            EXPECTED_UPGRADE_COSTS,
        );

        assert_eq!(new_wasm_config.initial_memory, 17);
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
        assert_eq!(upgrade0.new_deploy_config.unwrap().block_gas_limit, 38);

        let upgrade1 = &spec.upgrades[1];
        assert_eq!(upgrade1.activation_point, ActivationPoint { rank: 39 });
        assert_eq!(upgrade1.protocol_version, Version::from((0, 3, 0)));
        assert!(upgrade1.upgrade_installer_bytes.is_none());
        assert!(upgrade1.upgrade_installer_args.is_none());
        assert!(upgrade1.new_wasm_config.is_none());
        assert!(upgrade1.new_deploy_config.is_none());
    }

    #[test]
    fn check_bundled_spec() {
        let spec = Chainspec::from_resources("test/valid/chainspec.toml");
        check_spec(spec);
    }

    #[test]
    fn rmp_serde_roundtrip() {
        let mut rng = TestRng::new();
        let chainspec = Chainspec::random(&mut rng);
        testing::rmp_serde_roundtrip(&chainspec);
    }
}
