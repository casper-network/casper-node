//! Helper structs used to parse chainspec configuration files into their respective domain objects.

use std::{
    convert::{TryFrom, TryInto},
    path::Path,
    time::Duration,
};

use semver::Version;
use serde::{Deserialize, Serialize};

use casperlabs_types::U512;

use super::{chainspec, Error};
use crate::{
    components::contract_runtime::shared::wasm_costs::WasmCosts,
    types::Motes,
    utils::{read_file, External},
    GenesisAccount,
};

const DEFAULT_CHAIN_NAME: &str = "casperlabs-devnet";
const DEFAULT_MINT_INSTALLER_PATH: &str = "mint_install.wasm";
const DEFAULT_POS_INSTALLER_PATH: &str = "pos_install.wasm";
const DEFAULT_STANDARD_PAYMENT_INSTALLER_PATH: &str = "standard_payment_install.wasm";
const DEFAULT_ACCOUNTS_CSV_PATH: &str = "accounts.csv";
const DEFAULT_UPGRADE_INSTALLER_PATH: &str = "upgrade_install.wasm";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub(crate) struct DeployConfig {
    pub(crate) max_payment_cost: String,
    pub(crate) max_ttl_millis: u64,
    pub(crate) max_dependencies: u8,
    pub(crate) max_block_size: u32,
    pub(crate) block_gas_limit: u64,
}

impl Default for DeployConfig {
    fn default() -> Self {
        chainspec::DeployConfig::default().into()
    }
}

impl From<chainspec::DeployConfig> for DeployConfig {
    fn from(cfg: chainspec::DeployConfig) -> Self {
        DeployConfig {
            max_payment_cost: cfg.max_payment_cost.to_string(),
            max_ttl_millis: cfg.max_ttl.as_millis() as u64,
            max_dependencies: cfg.max_dependencies,
            max_block_size: cfg.max_block_size,
            block_gas_limit: cfg.block_gas_limit,
        }
    }
}

impl TryFrom<DeployConfig> for chainspec::DeployConfig {
    type Error = Error;

    fn try_from(cfg: DeployConfig) -> Result<Self, Self::Error> {
        let max_payment_cost = Motes::new(U512::from_dec_str(&cfg.max_payment_cost)?);
        Ok(chainspec::DeployConfig {
            max_payment_cost,
            max_ttl: Duration::from_millis(cfg.max_ttl_millis),
            max_dependencies: cfg.max_dependencies,
            max_block_size: cfg.max_block_size,
            block_gas_limit: cfg.block_gas_limit,
        })
    }
}

#[derive(PartialEq, Eq, Serialize, Deserialize, Debug)]
struct Genesis {
    name: String,
    timestamp: u64,
    protocol_version: Version,
    mint_installer_path: External<Vec<u8>>,
    pos_installer_path: External<Vec<u8>>,
    standard_payment_installer_path: External<Vec<u8>>,
    accounts_path: External<Vec<GenesisAccount>>,
}

impl Default for Genesis {
    fn default() -> Self {
        Genesis {
            name: String::from(DEFAULT_CHAIN_NAME),
            timestamp: 0,
            protocol_version: Version::from((1, 0, 0)),
            mint_installer_path: External::path(DEFAULT_MINT_INSTALLER_PATH),
            pos_installer_path: External::path(DEFAULT_POS_INSTALLER_PATH),
            standard_payment_installer_path: External::path(
                DEFAULT_STANDARD_PAYMENT_INSTALLER_PATH,
            ),
            accounts_path: External::path(DEFAULT_ACCOUNTS_CSV_PATH),
        }
    }
}

#[derive(PartialEq, Eq, Serialize, Deserialize, Debug)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
struct HighwayConfig {
    genesis_era_start_timestamp: u64,
    era_duration_millis: u64,
    booking_duration_millis: u64,
    entropy_duration_millis: u64,
    voting_period_duration_millis: u64,
    finality_threshold_percent: u8,
    minimum_round_exponent: u8,
}

impl Default for HighwayConfig {
    fn default() -> Self {
        chainspec::HighwayConfig::default().into()
    }
}

impl From<chainspec::HighwayConfig> for HighwayConfig {
    fn from(cfg: chainspec::HighwayConfig) -> Self {
        HighwayConfig {
            genesis_era_start_timestamp: cfg.genesis_era_start_timestamp,
            era_duration_millis: cfg.era_duration.as_millis() as u64,
            booking_duration_millis: cfg.booking_duration.as_millis() as u64,
            entropy_duration_millis: cfg.entropy_duration.as_millis() as u64,
            voting_period_duration_millis: cfg.voting_period_duration.as_millis() as u64,
            finality_threshold_percent: cfg.finality_threshold_percent,
            minimum_round_exponent: cfg.minimum_round_exponent,
        }
    }
}

#[derive(PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct UpgradePoint {
    pub(crate) protocol_version: Version,
    pub(crate) upgrade_installer_path: Option<External<Vec<u8>>>,
    pub(crate) activation_point: chainspec::ActivationPoint,
    pub(crate) new_costs: Option<WasmCosts>,
    pub(crate) new_deploy_config: Option<DeployConfig>,
}

impl From<&chainspec::UpgradePoint> for UpgradePoint {
    fn from(upgrade_point: &chainspec::UpgradePoint) -> Self {
        UpgradePoint {
            protocol_version: upgrade_point.protocol_version.clone(),
            upgrade_installer_path: Some(External::path(DEFAULT_UPGRADE_INSTALLER_PATH)),
            activation_point: upgrade_point.activation_point,
            new_costs: upgrade_point.new_costs,
            new_deploy_config: upgrade_point.new_deploy_config.map(DeployConfig::from),
        }
    }
}

/// A chainspec configuration as laid out in the configuration file.
#[derive(Default, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(super) struct ChainspecConfig {
    genesis: Genesis,
    highway: HighwayConfig,
    deploys: DeployConfig,
    wasm_costs: WasmCosts,
    upgrade: Option<Vec<UpgradePoint>>,
}

impl From<&chainspec::Chainspec> for ChainspecConfig {
    fn from(chainspec: &chainspec::Chainspec) -> Self {
        let genesis = Genesis {
            name: chainspec.genesis.name.clone(),
            timestamp: chainspec.genesis.timestamp,
            protocol_version: chainspec.genesis.protocol_version.clone(),
            mint_installer_path: External::path(DEFAULT_MINT_INSTALLER_PATH),
            pos_installer_path: External::path(DEFAULT_POS_INSTALLER_PATH),
            standard_payment_installer_path: External::path(
                DEFAULT_STANDARD_PAYMENT_INSTALLER_PATH,
            ),
            accounts_path: External::path(DEFAULT_ACCOUNTS_CSV_PATH),
        };

        let highway = HighwayConfig {
            genesis_era_start_timestamp: chainspec
                .genesis
                .highway_config
                .genesis_era_start_timestamp,
            era_duration_millis: chainspec.genesis.highway_config.era_duration.as_millis() as u64,
            booking_duration_millis: chainspec
                .genesis
                .highway_config
                .booking_duration
                .as_millis() as u64,
            entropy_duration_millis: chainspec
                .genesis
                .highway_config
                .entropy_duration
                .as_millis() as u64,
            voting_period_duration_millis: chainspec
                .genesis
                .highway_config
                .voting_period_duration
                .as_millis() as u64,
            finality_threshold_percent: chainspec.genesis.highway_config.finality_threshold_percent,
            minimum_round_exponent: chainspec.genesis.highway_config.minimum_round_exponent,
        };

        let deploys = chainspec.genesis.deploy_config.into();
        let wasm_costs = chainspec.genesis.costs;

        let upgrades = chainspec
            .upgrades
            .iter()
            .map(UpgradePoint::from)
            .collect::<Vec<_>>();
        let upgrade = if upgrades.is_empty() {
            None
        } else {
            Some(upgrades)
        };

        ChainspecConfig {
            genesis,
            highway,
            deploys,
            wasm_costs,
            upgrade,
        }
    }
}

pub(super) fn parse_toml<P: AsRef<Path>>(chainspec_path: P) -> Result<chainspec::Chainspec, Error> {
    let chainspec: ChainspecConfig =
        toml::from_slice(&read_file(chainspec_path.as_ref()).map_err(Error::LoadChainspec)?)?;

    let root = chainspec_path
        .as_ref()
        .parent()
        .unwrap_or_else(|| Path::new(""));

    let mint_installer_bytes = chainspec
        .genesis
        .mint_installer_path
        .load_relative(root)
        .map_err(Error::LoadMintInstaller)?;

    let pos_installer_bytes = chainspec
        .genesis
        .pos_installer_path
        .load_relative(root)
        .map_err(Error::LoadPosInstaller)?;

    let standard_payment_installer_bytes = chainspec
        .genesis
        .standard_payment_installer_path
        .load_relative(root)
        .map_err(Error::LoadStandardPaymentInstaller)?;

    let accounts: Vec<GenesisAccount> = chainspec
        .genesis
        .accounts_path
        .load_relative(root)
        .map_err(Error::LoadGenesisAccounts)?;
    let highway_config = chainspec::HighwayConfig {
        genesis_era_start_timestamp: chainspec.highway.genesis_era_start_timestamp,
        era_duration: Duration::from_millis(chainspec.highway.era_duration_millis),
        booking_duration: Duration::from_millis(chainspec.highway.booking_duration_millis),
        entropy_duration: Duration::from_millis(chainspec.highway.entropy_duration_millis),
        voting_period_duration: Duration::from_millis(
            chainspec.highway.voting_period_duration_millis,
        ),
        finality_threshold_percent: chainspec.highway.finality_threshold_percent,
        minimum_round_exponent: chainspec.highway.minimum_round_exponent,
    };

    let genesis = chainspec::GenesisConfig {
        name: chainspec.genesis.name,
        timestamp: chainspec.genesis.timestamp,
        protocol_version: chainspec.genesis.protocol_version,
        mint_installer_bytes,
        pos_installer_bytes,
        standard_payment_installer_bytes,
        accounts,
        costs: chainspec.wasm_costs,
        deploy_config: chainspec.deploys.try_into()?,
        highway_config,
    };

    let mut upgrades = vec![];
    for upgrade_point in chainspec.upgrade.unwrap_or_default().into_iter() {
        upgrades.push(chainspec::UpgradePoint::try_from_config(
            root,
            upgrade_point,
        )?);
    }

    Ok(chainspec::Chainspec { genesis, upgrades })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{chainspec::rewrite_with_absolute_paths, *};

    const PRODUCTION_DIR: &str = "resources/production";
    const LOCAL_DIR: &str = "resources/local";
    const TARGET_DIR: &str = "target/wasm32-unknown-unknown/release";
    const MINT: &str = "mint_install.wasm";
    const POS: &str = "pos_install.wasm";
    const STANDARD_PAYMENT: &str = "standard_payment_install.wasm";
    const CHAINSPEC_CONFIG_NAME: &str = "chainspec.toml";

    #[test]
    fn default_config_should_match_production() {
        let default = ChainspecConfig::default();
        let production_dir = format!("{}/../{}", env!("CARGO_MANIFEST_DIR"), PRODUCTION_DIR);
        let chainspec_config = rewrite_with_absolute_paths(&production_dir);

        let production = ChainspecConfig::from(&parse_toml(chainspec_config.path()).unwrap());
        assert_eq!(production, default);
    }

    #[test]
    fn local_chainspec_should_parse() {
        let mint = PathBuf::from(format!(
            "{}/{}/{}",
            env!("CARGO_MANIFEST_DIR"),
            TARGET_DIR,
            MINT
        ));
        let pos = PathBuf::from(format!(
            "{}/{}/{}",
            env!("CARGO_MANIFEST_DIR"),
            TARGET_DIR,
            POS
        ));
        let standard_payment = PathBuf::from(format!(
            "{}/{}/{}",
            env!("CARGO_MANIFEST_DIR"),
            TARGET_DIR,
            STANDARD_PAYMENT
        ));
        if !mint.exists() || !pos.exists() || !standard_payment.exists() {
            // We can't test if the Wasm files are missing.
            return;
        }

        let local_path = format!(
            "{}/../{}/{}",
            env!("CARGO_MANIFEST_DIR"),
            LOCAL_DIR,
            CHAINSPEC_CONFIG_NAME
        );
        let _chainspec = ChainspecConfig::from(&parse_toml(local_path).unwrap());
    }
}
