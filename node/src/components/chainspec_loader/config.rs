//! Helper structs used to parse chainspec configuration files into their respective domain objects.

use std::path::Path;

use semver::Version;
use serde::{Deserialize, Serialize};

use casper_execution_engine::{
    core::engine_state::genesis::GenesisAccount, shared::wasm_costs::WasmCosts,
};

use super::{chainspec, Error};
use crate::{
    types::Timestamp,
    utils::{read_file, External},
};

const DEFAULT_CHAIN_NAME: &str = "casper-devnet";
const DEFAULT_MINT_INSTALLER_PATH: &str = "mint_install.wasm";
const DEFAULT_POS_INSTALLER_PATH: &str = "pos_install.wasm";
const DEFAULT_STANDARD_PAYMENT_INSTALLER_PATH: &str = "standard_payment_install.wasm";
const DEFAULT_AUCTION_INSTALLER_PATH: &str = "auction_install.wasm";
const DEFAULT_ACCOUNTS_CSV_PATH: &str = "accounts.csv";
const DEFAULT_UPGRADE_INSTALLER_PATH: &str = "upgrade_install.wasm";

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
#[serde(deny_unknown_fields)]
struct UpgradePoint {
    protocol_version: Version,
    upgrade_installer_path: Option<External<Vec<u8>>>,
    activation_point: chainspec::ActivationPoint,
    new_costs: Option<WasmCosts>,
    new_deploy_config: Option<chainspec::DeployConfig>,
}

impl From<&chainspec::UpgradePoint> for UpgradePoint {
    fn from(upgrade_point: &chainspec::UpgradePoint) -> Self {
        UpgradePoint {
            protocol_version: upgrade_point.protocol_version.clone(),
            upgrade_installer_path: Some(External::path(DEFAULT_UPGRADE_INSTALLER_PATH)),
            activation_point: upgrade_point.activation_point,
            new_costs: upgrade_point.new_costs,
            new_deploy_config: upgrade_point.new_deploy_config,
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

        Ok(chainspec::UpgradePoint {
            activation_point: self.activation_point,
            protocol_version: self.protocol_version,
            upgrade_installer_bytes,
            upgrade_installer_args,
            new_costs: self.new_costs,
            new_deploy_config: self.new_deploy_config,
        })
    }
}

/// A chainspec configuration as laid out in the configuration file.
#[derive(Default, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(super) struct ChainspecConfig {
    genesis: Genesis,
    highway: chainspec::HighwayConfig,
    deploys: chainspec::DeployConfig,
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

        let highway = chainspec.genesis.highway_config;
        let deploys = chainspec.genesis.deploy_config;
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
        deploy_config: chainspec.deploys,
        highway_config: chainspec.highway,
    };

    let mut upgrades = vec![];
    for upgrade_point in chainspec.upgrade.unwrap_or_default().into_iter() {
        upgrades.push(upgrade_point.try_into_chainspec_upgrade_point(root)?);
    }

    Ok(chainspec::Chainspec { genesis, upgrades })
}
