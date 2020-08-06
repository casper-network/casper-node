//! The chainspec component.  Responsible for reading in the chainspec configuration file on app
//! start, and putting the parsed value into persistent storage.
//!
//! See https://casperlabs.atlassian.net/wiki/spaces/EN/pages/135528449/Genesis+Process+Specification
//! for full details.

use std::{
    fmt::{self, Debug, Formatter},
    path::Path,
    time::Duration,
};

use num_traits::Zero;
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
use semver::Version;
use serde::{Deserialize, Serialize};

use casperlabs_types::{account::AccountHash, U512};

use super::{config, Error};
use crate::{
    components::contract_runtime::shared::wasm_costs::WasmCosts, crypto::asymmetric_key::PublicKey,
    types::Motes,
};
#[cfg(test)]
use crate::{testing::TestRng, types::Timestamp};

/// An account that exists at genesis.
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct GenesisAccount {
    account_hash: AccountHash,
    public_key: Option<PublicKey>,
    balance: Motes,
    bonded_amount: Motes,
}

impl GenesisAccount {
    /// Constructs a new `GenesisAccount` with no public key.
    pub fn new(account_hash: AccountHash, balance: Motes, bonded_amount: Motes) -> Self {
        GenesisAccount {
            public_key: None,
            account_hash,
            balance,
            bonded_amount,
        }
    }

    /// Constructs a new `GenesisAccount` with a given public key.
    pub fn with_public_key(public_key: PublicKey, balance: Motes, bonded_amount: Motes) -> Self {
        let account_hash = public_key.to_account_hash();
        GenesisAccount {
            public_key: Some(public_key),
            account_hash,
            balance,
            bonded_amount,
        }
    }

    /// Returns the account's public key.
    pub fn public_key(&self) -> Option<PublicKey> {
        self.public_key
    }

    /// Returns the account's hash.
    pub fn account_hash(&self) -> AccountHash {
        self.account_hash
    }

    /// Returns the account's balance.
    pub fn balance(&self) -> Motes {
        self.balance
    }

    /// Returns the account's bonded amount.
    pub fn bonded_amount(&self) -> Motes {
        self.bonded_amount
    }
}

impl Distribution<GenesisAccount> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> GenesisAccount {
        let public_key = None;
        let account_hash = AccountHash::new(rng.gen());
        let balance = Motes::new(U512(rng.gen()));
        let bonded_amount = Motes::new(U512(rng.gen()));

        GenesisAccount {
            public_key,
            account_hash,
            balance,
            bonded_amount,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub(crate) struct DeployConfig {
    pub(crate) max_payment_cost: Motes,
    pub(crate) max_ttl: Duration,
    pub(crate) max_dependencies: u8,
    pub(crate) max_block_size: u32,
    pub(crate) block_gas_limit: u64,
}

impl Default for DeployConfig {
    fn default() -> Self {
        DeployConfig {
            max_payment_cost: Motes::zero(),
            max_ttl: Duration::from_millis(86_400_000), // 1 day
            max_dependencies: 10,
            max_block_size: 10_485_760,
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
        let max_ttl = Duration::from_millis(rng.gen_range(60_000, 3_600_000));
        let max_dependencies = rng.gen();
        let max_block_size = rng.gen_range(1_000_000, 1_000_000_000);
        let block_gas_limit = rng.gen_range(100_000_000_000, 1_000_000_000_000_000);

        DeployConfig {
            max_payment_cost,
            max_ttl,
            max_dependencies,
            max_block_size,
            block_gas_limit,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub(crate) struct HighwayConfig {
    pub(crate) genesis_era_start_timestamp: u64,
    pub(crate) era_duration: Duration,
    pub(crate) booking_duration: Duration,
    pub(crate) entropy_duration: Duration,
    pub(crate) voting_period_duration: Duration,
    pub(crate) finality_threshold_percent: u8,
}

impl Default for HighwayConfig {
    fn default() -> Self {
        HighwayConfig {
            genesis_era_start_timestamp: 1_583_712_000_000,
            era_duration: Duration::from_millis(604_800_000), // 1 week
            booking_duration: Duration::from_millis(864_000_000), // 10 days
            entropy_duration: Duration::from_millis(10_800_000), // 3 hours
            voting_period_duration: Duration::from_millis(172_800_000), // 2 days
            finality_threshold_percent: 10,
        }
    }
}

#[cfg(test)]
impl HighwayConfig {
    /// Generates a random instance using a `TestRng`.
    pub fn random(rng: &mut TestRng) -> Self {
        let genesis_era_start_timestamp = Timestamp::now().millis();
        let era_duration = Duration::from_millis(rng.gen_range(600_000, 604_800_000));
        let booking_duration = Duration::from_millis(rng.gen_range(600_000, 864_000_000));
        let entropy_duration = Duration::from_millis(rng.gen_range(600_000, 10_800_000));
        let voting_period_duration = Duration::from_millis(rng.gen_range(600_000, 172_800_000));
        let finality_threshold_percent = rng.gen_range(0, 101);

        HighwayConfig {
            genesis_era_start_timestamp,
            era_duration,
            booking_duration,
            entropy_duration,
            voting_period_duration,
            finality_threshold_percent,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub(crate) struct GenesisConfig {
    pub(crate) name: String,
    pub(crate) timestamp: u64,
    pub(crate) protocol_version: Version,
    pub(crate) mint_installer_bytes: Vec<u8>,
    pub(crate) pos_installer_bytes: Vec<u8>,
    pub(crate) standard_payment_installer_bytes: Vec<u8>,
    pub(crate) auction_installer_bytes: Vec<u8>,
    pub(crate) accounts: Vec<GenesisAccount>,
    pub(crate) costs: WasmCosts,
    pub(crate) deploy_config: DeployConfig,
    pub(crate) highway_config: HighwayConfig,
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
            .field("costs", &self.costs)
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
        let timestamp = rng.gen::<u8>() as u64;
        let protocol_version = Version::new(
            rng.gen_range(0, 10),
            rng.gen::<u8>() as u64,
            rng.gen::<u8>() as u64,
        );
        let mint_installer_bytes = vec![rng.gen()];
        let pos_installer_bytes = vec![rng.gen()];
        let standard_payment_installer_bytes = vec![rng.gen()];
        let accounts = vec![rng.gen(), rng.gen(), rng.gen(), rng.gen(), rng.gen()];
        let costs = WasmCosts::random(rng);
        let deploy_config = DeployConfig::random(rng);
        let highway_config = HighwayConfig::random(rng);

        GenesisConfig {
            name,
            timestamp,
            protocol_version,
            mint_installer_bytes,
            pos_installer_bytes,
            standard_payment_installer_bytes,
            accounts,
            costs,
            deploy_config,
            highway_config,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ActivationPoint {
    pub(crate) rank: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct UpgradePoint {
    pub(crate) activation_point: ActivationPoint,
    pub(crate) protocol_version: Version,
    pub(crate) upgrade_installer_bytes: Option<Vec<u8>>,
    pub(crate) upgrade_installer_args: Option<Vec<u8>>,
    pub(crate) new_costs: Option<WasmCosts>,
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
        let new_costs = if rng.gen() {
            Some(WasmCosts::random(rng))
        } else {
            None
        };
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
            new_costs,
            new_deploy_config,
        }
    }
}

/// A collection of configuration settings describing the state of the system at genesis and
/// upgrades to basic system functionality (including system contracts and gas costs) occurring
/// after genesis.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chainspec {
    pub(crate) genesis: GenesisConfig,
    pub(crate) upgrades: Vec<UpgradePoint>,
}

impl Chainspec {
    /// Converts the chainspec to a TOML-formatted string.
    pub fn to_toml(&self) -> Result<String, Error> {
        let config = config::Chainspec::from(self);
        toml::to_string_pretty(&config).map_err(Error::EncodingToToml)
    }

    /// Reads and parses the chainspec configuration files specified in `chainspec_path`.
    ///
    /// `chainspec_path` should refer to a TOML-formatted chainspec configuration file (generally
    /// named "chainspec.toml").  This file can specify paths to further required files.  Each of
    /// these paths can either be absolute or relative to the folder containing `chainspec_path`.
    pub fn from_toml<P: AsRef<Path>>(chainspec_path: P) -> Result<Self, Error> {
        config::parse_toml(chainspec_path)
    }

    /// Generates a random instance using a `TestRng`.
    #[cfg(test)]
    pub fn random(rng: &mut TestRng) -> Self {
        let genesis = GenesisConfig::random(rng);
        let upgrades = vec![UpgradePoint::random(rng), UpgradePoint::random(rng)];
        Chainspec { genesis, upgrades }
    }

    /// Constructs a chainspec from the chainspec.toml in 'resources/local'.
    #[cfg(test)]
    pub fn from_resources_local() -> Self {
        const PATH: &str = "resources/local/chainspec.toml";

        let path = format!("{}/../{}", env!("CARGO_MANIFEST_DIR"), PATH);
        Chainspec::from_toml(path).unwrap()
    }
}

#[cfg(test)]
pub(super) use tests::rewrite_with_absolute_paths;

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::NamedTempFile;

    use super::*;
    use crate::utils::read_file_to_string;

    const TEST_ROOT: &str = "resources/test/valid";
    const CHAINSPEC_CONFIG_NAME: &str = "chainspec.toml";

    /// Takes a chainspec.toml in the specified `chainspec_dir` and rewrites it to a temp file with
    /// the relative paths rewritten to absolute paths.
    pub(in crate::components::chainspec_handler) fn rewrite_with_absolute_paths(
        chainspec_dir: &str,
    ) -> NamedTempFile {
        let original_contents =
            read_file_to_string(format!("{}/{}", chainspec_dir, CHAINSPEC_CONFIG_NAME)).unwrap();

        // Replace relative paths with absolute ones.
        let test_root = format!("{}/../{}", env!("CARGO_MANIFEST_DIR"), TEST_ROOT,);
        let mut updated_contents = String::new();
        for line in original_contents.lines() {
            let updated_line = if line.starts_with("mint_installer_path") {
                format!("mint_installer_path = '{}/mint.wasm'", test_root)
            } else if line.starts_with("pos_installer_path") {
                format!("pos_installer_path = '{}/pos.wasm'", test_root)
            } else if line.starts_with("standard_payment_installer_path") {
                format!(
                    "standard_payment_installer_path = '{}/standard_payment.wasm'",
                    test_root
                )
            } else if line.starts_with("auction_installer_path") {
                format!(
                    "auction_installer_path = '{}/auction_install.wasm'",
                    test_root
                )
            } else if line.starts_with("accounts_path") {
                format!("accounts_path = '{}/accounts.csv'", test_root)
            } else if line.starts_with("upgrade_installer_path") {
                format!("upgrade_installer_path = '{}/upgrade.wasm'", test_root)
            } else {
                line.to_string()
            };
            updated_contents.push_str(&updated_line);
            updated_contents.push('\n');
        }

        // Write the updated file to a temporary file which will be deleted on test exit.
        let mut chainspec_config = NamedTempFile::new().unwrap();
        chainspec_config
            .write_all(updated_contents.as_bytes())
            .unwrap();
        chainspec_config
    }

    fn check_spec(spec: Chainspec) {
        assert_eq!(spec.genesis.name, "test-chain");
        assert_eq!(spec.genesis.timestamp, 1);
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
                spec.genesis.accounts[index].balance,
                Motes::new(U512::from(index + 1))
            );
            assert_eq!(
                spec.genesis.accounts[index].bonded_amount,
                Motes::new(U512::from((index as u64 + 1) * 10))
            );
        }

        assert_eq!(spec.genesis.highway_config.genesis_era_start_timestamp, 2);
        assert_eq!(
            spec.genesis.highway_config.era_duration,
            Duration::from_millis(3)
        );
        assert_eq!(
            spec.genesis.highway_config.booking_duration,
            Duration::from_millis(4)
        );
        assert_eq!(
            spec.genesis.highway_config.entropy_duration,
            Duration::from_millis(5)
        );
        assert_eq!(
            spec.genesis.highway_config.voting_period_duration,
            Duration::from_millis(6)
        );
        assert_eq!(spec.genesis.highway_config.finality_threshold_percent, 8);

        assert_eq!(
            spec.genesis.deploy_config.max_payment_cost,
            Motes::new(U512::from(9))
        );
        assert_eq!(
            spec.genesis.deploy_config.max_ttl,
            Duration::from_millis(10)
        );
        assert_eq!(spec.genesis.deploy_config.max_dependencies, 11);
        assert_eq!(spec.genesis.deploy_config.max_block_size, 12);
        assert_eq!(spec.genesis.deploy_config.block_gas_limit, 13);

        assert_eq!(spec.genesis.costs.regular, 13);
        assert_eq!(spec.genesis.costs.div, 14);
        assert_eq!(spec.genesis.costs.mul, 15);
        assert_eq!(spec.genesis.costs.mem, 16);
        assert_eq!(spec.genesis.costs.initial_mem, 17);
        assert_eq!(spec.genesis.costs.grow_mem, 18);
        assert_eq!(spec.genesis.costs.memcpy, 19);
        assert_eq!(spec.genesis.costs.max_stack_height, 20);
        assert_eq!(spec.genesis.costs.opcodes_mul, 21);
        assert_eq!(spec.genesis.costs.opcodes_div, 22);

        assert_eq!(spec.upgrades.len(), 2);

        let upgrade0 = &spec.upgrades[0];
        assert_eq!(upgrade0.activation_point, ActivationPoint { rank: 23 });
        assert_eq!(upgrade0.protocol_version, Version::from((0, 2, 0)));
        assert_eq!(
            upgrade0.upgrade_installer_bytes,
            Some(b"Upgrade installer bytes".to_vec())
        );
        assert!(upgrade0.upgrade_installer_args.is_none());
        assert_eq!(upgrade0.new_costs.unwrap().regular, 24);
        assert_eq!(upgrade0.new_costs.unwrap().div, 25);
        assert_eq!(upgrade0.new_costs.unwrap().mul, 26);
        assert_eq!(upgrade0.new_costs.unwrap().mem, 27);
        assert_eq!(upgrade0.new_costs.unwrap().initial_mem, 28);
        assert_eq!(upgrade0.new_costs.unwrap().grow_mem, 29);
        assert_eq!(upgrade0.new_costs.unwrap().memcpy, 30);
        assert_eq!(upgrade0.new_costs.unwrap().max_stack_height, 31);
        assert_eq!(upgrade0.new_costs.unwrap().opcodes_mul, 32);
        assert_eq!(upgrade0.new_costs.unwrap().opcodes_div, 33);
        assert_eq!(
            upgrade0.new_deploy_config.unwrap().max_payment_cost,
            Motes::new(U512::from(34))
        );
        assert_eq!(
            upgrade0.new_deploy_config.unwrap().max_ttl,
            Duration::from_millis(35)
        );
        assert_eq!(upgrade0.new_deploy_config.unwrap().max_dependencies, 36);
        assert_eq!(upgrade0.new_deploy_config.unwrap().max_block_size, 37);
        assert_eq!(upgrade0.new_deploy_config.unwrap().block_gas_limit, 38);

        let upgrade1 = &spec.upgrades[1];
        assert_eq!(upgrade1.activation_point, ActivationPoint { rank: 39 });
        assert_eq!(upgrade1.protocol_version, Version::from((0, 3, 0)));
        assert!(upgrade1.upgrade_installer_bytes.is_none());
        assert!(upgrade1.upgrade_installer_args.is_none());
        assert!(upgrade1.new_costs.is_none());
        assert!(upgrade1.new_deploy_config.is_none());
    }

    #[test]
    fn should_read_relative_paths() {
        let path = format!(
            "{}/../{}/{}",
            env!("CARGO_MANIFEST_DIR"),
            TEST_ROOT,
            CHAINSPEC_CONFIG_NAME
        );
        let spec = Chainspec::from_toml(path).unwrap();
        check_spec(spec);
    }

    #[test]
    fn should_read_absolute_paths() {
        let test_root = format!("{}/../{}", env!("CARGO_MANIFEST_DIR"), TEST_ROOT,);
        let chainspec_config = rewrite_with_absolute_paths(&test_root);

        // Check the parsed chainspec.
        let spec = Chainspec::from_toml(chainspec_config.path()).unwrap();
        check_spec(spec);
    }
}
