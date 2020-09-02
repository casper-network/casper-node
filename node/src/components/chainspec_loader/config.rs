//! Helper structs used to parse chainspec configuration files into their respective domain objects.

use std::{
    convert::{TryFrom, TryInto},
    path::Path,
};

use semver::Version;
use serde::{Deserialize, Serialize};

use casper_types::U512;

use super::{
    chainspec::{self, GenesisAccount},
    Error,
};
use crate::{
    components::contract_runtime::shared::wasm_costs::WasmCosts,
    types::{Motes, TimeDiff, Timestamp},
    utils::{read_file, External},
};

const DEFAULT_CHAIN_NAME: &str = "casper-devnet";
const DEFAULT_MINT_INSTALLER_PATH: &str = "mint_install.wasm";
const DEFAULT_POS_INSTALLER_PATH: &str = "pos_install.wasm";
const DEFAULT_STANDARD_PAYMENT_INSTALLER_PATH: &str = "standard_payment_install.wasm";
const DEFAULT_AUCTION_INSTALLER_PATH: &str = "auction_install.wasm";
const DEFAULT_ACCOUNTS_CSV_PATH: &str = "accounts.csv";
const DEFAULT_UPGRADE_INSTALLER_PATH: &str = "upgrade_install.wasm";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct DeployConfig {
    max_payment_cost: String,
    max_ttl_millis: TimeDiff,
    max_dependencies: u8,
    max_block_size: u32,
    block_gas_limit: u64,
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
            max_ttl_millis: cfg.max_ttl,
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
            max_ttl: cfg.max_ttl_millis,
            max_dependencies: cfg.max_dependencies,
            max_block_size: cfg.max_block_size,
            block_gas_limit: cfg.block_gas_limit,
        })
    }
}

#[derive(PartialEq, Eq, Serialize, Deserialize, Debug)]
struct Genesis {
    name: String,
    timestamp: Timestamp,
    protocol_version: Version,
    mint_installer_path: External<Vec<u8>>,
    pos_installer_path: External<Vec<u8>>,
    standard_payment_installer_path: External<Vec<u8>>,
    auction_installer_path: External<Vec<u8>>,
    accounts_path: External<Vec<GenesisAccount>>,
}

impl Default for Genesis {
    fn default() -> Self {
        Genesis {
            name: String::from(DEFAULT_CHAIN_NAME),
            timestamp: Timestamp::zero(),
            protocol_version: Version::from((1, 0, 0)),
            mint_installer_path: External::path(DEFAULT_MINT_INSTALLER_PATH),
            pos_installer_path: External::path(DEFAULT_POS_INSTALLER_PATH),
            standard_payment_installer_path: External::path(
                DEFAULT_STANDARD_PAYMENT_INSTALLER_PATH,
            ),
            auction_installer_path: External::path(DEFAULT_AUCTION_INSTALLER_PATH),
            accounts_path: External::path(DEFAULT_ACCOUNTS_CSV_PATH),
        }
    }
}

#[derive(PartialEq, Eq, Serialize, Deserialize, Debug)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
struct HighwayConfig {
    genesis_era_start_timestamp: Timestamp,
    era_duration_millis: u64,
    minimum_era_height: u64,
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
            era_duration_millis: cfg.era_duration.millis(),
            minimum_era_height: cfg.minimum_era_height,
            booking_duration_millis: cfg.booking_duration.millis(),
            entropy_duration_millis: cfg.entropy_duration.millis(),
            voting_period_duration_millis: cfg.voting_period_duration.millis(),
            finality_threshold_percent: cfg.finality_threshold_percent,
            minimum_round_exponent: cfg.minimum_round_exponent,
        }
    }
}

#[derive(PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct UpgradePoint {
    protocol_version: Version,
    upgrade_installer_path: Option<External<Vec<u8>>>,
    activation_point: chainspec::ActivationPoint,
    new_costs: Option<WasmCosts>,
    new_deploy_config: Option<DeployConfig>,
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

impl UpgradePoint {
    fn try_into_chainspec_upgrade_point<P: AsRef<Path>>(
        self,
        root: P,
    ) -> Result<chainspec::UpgradePoint, Error> {
        let upgrade_installer_bytes = self
            .upgrade_installer_path
            .map(|ext_vec| ext_vec.load(root.as_ref()))
            .transpose()
            .map_err(Error::LoadUpgradeInstaller)?;
        // TODO - read this in?
        let upgrade_installer_args = None;

        let new_deploy_config = self
            .new_deploy_config
            .map(DeployConfig::try_into)
            .transpose()?;
        Ok(chainspec::UpgradePoint {
            activation_point: self.activation_point,
            protocol_version: self.protocol_version,
            upgrade_installer_bytes,
            upgrade_installer_args,
            new_costs: self.new_costs,
            new_deploy_config,
        })
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
            auction_installer_path: External::path(DEFAULT_AUCTION_INSTALLER_PATH),
            accounts_path: External::path(DEFAULT_ACCOUNTS_CSV_PATH),
        };

        let highway = HighwayConfig {
            genesis_era_start_timestamp: chainspec
                .genesis
                .highway_config
                .genesis_era_start_timestamp,
            era_duration_millis: chainspec.genesis.highway_config.era_duration.millis(),
            minimum_era_height: chainspec.genesis.highway_config.minimum_era_height,
            booking_duration_millis: chainspec.genesis.highway_config.booking_duration.millis(),
            entropy_duration_millis: chainspec.genesis.highway_config.entropy_duration.millis(),
            voting_period_duration_millis: chainspec
                .genesis
                .highway_config
                .voting_period_duration
                .millis(),
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
        .load(root)
        .map_err(Error::LoadMintInstaller)?;

    let pos_installer_bytes = chainspec
        .genesis
        .pos_installer_path
        .load(root)
        .map_err(Error::LoadPosInstaller)?;

    let standard_payment_installer_bytes = chainspec
        .genesis
        .standard_payment_installer_path
        .load(root)
        .map_err(Error::LoadStandardPaymentInstaller)?;

    let auction_installer_bytes = chainspec
        .genesis
        .auction_installer_path
        .load(root)
        .map_err(Error::LoadAuctionInstaller)?;

    let accounts: Vec<GenesisAccount> = chainspec
        .genesis
        .accounts_path
        .load(root)
        .map_err(Error::LoadGenesisAccounts)?;
    let highway_config = chainspec::HighwayConfig {
        genesis_era_start_timestamp: chainspec.highway.genesis_era_start_timestamp,
        era_duration: TimeDiff::from(chainspec.highway.era_duration_millis),
        minimum_era_height: chainspec.highway.minimum_era_height,
        booking_duration: TimeDiff::from(chainspec.highway.booking_duration_millis),
        entropy_duration: TimeDiff::from(chainspec.highway.entropy_duration_millis),
        voting_period_duration: TimeDiff::from(chainspec.highway.voting_period_duration_millis),
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
        auction_installer_bytes,
        accounts,
        costs: chainspec.wasm_costs,
        deploy_config: chainspec.deploys.try_into()?,
        highway_config,
    };

    let mut upgrades = vec![];
    for upgrade_point in chainspec.upgrade.unwrap_or_default().into_iter() {
        upgrades.push(upgrade_point.try_into_chainspec_upgrade_point(root)?);
    }

    Ok(chainspec::Chainspec { genesis, upgrades })
}
