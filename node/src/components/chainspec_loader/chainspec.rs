use std::{
    convert::TryInto,
    fmt::{self, Debug, Formatter},
    path::Path,
    time::Duration,
};

use csv::ReaderBuilder;
use num_traits::Zero;
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
use semver::Version;
use serde::{Deserialize, Serialize};

use casperlabs_types::{account::AccountHash, U512};

use super::{config, error::GenesisLoadError, Error};
#[cfg(test)]
use crate::types::Timestamp;
use crate::{
    components::contract_runtime::shared::wasm_costs::WasmCosts, crypto::asymmetric_key::PublicKey,
    types::Motes, utils::Loadable,
};

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

impl Loadable for Vec<GenesisAccount> {
    type Error = GenesisLoadError;

    fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, Self::Error> {
        #[derive(Debug, Deserialize)]
        struct ParsedAccount {
            public_key: String,
            algorithm: String,
            balance: String,
            bonded_amount: String,
        }

        let mut reader = ReaderBuilder::new().has_headers(false).from_path(path)?;
        let mut accounts = vec![];
        for result in reader.deserialize() {
            let parsed: ParsedAccount = result?;
            let balance = Motes::new(U512::from_dec_str(&parsed.balance)?);
            let bonded_amount = Motes::new(U512::from_dec_str(&parsed.bonded_amount)?);
            let key_bytes = hex::decode(parsed.public_key)?;
            let account = GenesisAccount::with_public_key(
                PublicKey::key_from_algorithm_name_and_bytes(&parsed.algorithm, key_bytes)?,
                balance,
                bonded_amount,
            );
            accounts.push(account);
        }
        Ok(accounts)
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
impl Distribution<DeployConfig> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> DeployConfig {
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
    pub(crate) minimum_round_exponent: u8,
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
            minimum_round_exponent: 14, // 2**14 ms = ~16 seconds
        }
    }
}

#[cfg(test)]
impl Distribution<HighwayConfig> for Standard {
    /// Generates a random instance.
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> HighwayConfig {
        let genesis_era_start_timestamp = Timestamp::now().millis();
        let era_duration = Duration::from_millis(rng.gen_range(600_000, 604_800_000));
        let booking_duration = Duration::from_millis(rng.gen_range(600_000, 864_000_000));
        let entropy_duration = Duration::from_millis(rng.gen_range(600_000, 10_800_000));
        let voting_period_duration = Duration::from_millis(rng.gen_range(600_000, 172_800_000));
        let finality_threshold_percent = rng.gen_range(0, 101);
        let minimum_round_exponent = rng.gen_range(0, 20);

        HighwayConfig {
            genesis_era_start_timestamp,
            era_duration,
            booking_duration,
            entropy_duration,
            voting_period_duration,
            finality_threshold_percent,
            minimum_round_exponent,
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
impl Distribution<GenesisConfig> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> GenesisConfig {
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
        let costs = rng.gen();
        let deploy_config = rng.gen();
        let highway_config = rng.gen();

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

impl UpgradePoint {
    pub(crate) fn try_from_config<P: AsRef<Path>>(
        root: P,
        cfg_upgrade_point: config::UpgradePoint,
    ) -> Result<Self, Error> {
        let upgrade_installer_bytes = cfg_upgrade_point
            .upgrade_installer_path
            .map(|ext_vec| ext_vec.load_relative(root.as_ref()))
            .transpose()
            .map_err(Error::LoadUpgradeInstaller)?;
        // TODO - read this in?
        let upgrade_installer_args = None;

        let new_deploy_config = cfg_upgrade_point
            .new_deploy_config
            .map(config::DeployConfig::try_into)
            .transpose()?;
        Ok(UpgradePoint {
            activation_point: cfg_upgrade_point.activation_point,
            protocol_version: cfg_upgrade_point.protocol_version,
            upgrade_installer_bytes,
            upgrade_installer_args,
            new_costs: cfg_upgrade_point.new_costs,
            new_deploy_config,
        })
    }
}

#[cfg(test)]
impl UpgradePoint {
    /// Generates a random instance using a `TestRng`.
    pub fn random<R: Rng + ?Sized>(rng: &mut R) -> Self {
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
        let new_deploy_config = if rng.gen() { Some(rng.gen()) } else { None };

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

impl Loadable for Chainspec {
    type Error = Error;
    fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, Self::Error> {
        config::parse_toml(path)
    }
}

#[cfg(test)]
impl Distribution<Chainspec> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Chainspec {
        let genesis = rng.gen();
        let upgrades = vec![UpgradePoint::random(rng), UpgradePoint::random(rng)];
        Chainspec { genesis, upgrades }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{self, TestRng};

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
        assert_eq!(spec.genesis.highway_config.minimum_round_exponent, 13);

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
    fn check_bundled_spec() {
        let spec = Chainspec::bundled("test/valid/chainspec.toml");
        check_spec(spec);
    }

    #[test]
    fn rmp_serde_roundtrip() {
        let mut rng = TestRng::new();
        let chainspec: Chainspec = rng.gen();
        testing::rmp_serde_roundtrip(&chainspec);
    }
}
