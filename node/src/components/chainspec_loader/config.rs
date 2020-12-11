//! Helper structs used to parse chainspec configuration files into their respective domain objects.

use std::path::Path;

use num_rational::Ratio;
use semver::Version;
use serde::{Deserialize, Serialize};

use casper_execution_engine::{
    core::engine_state::genesis::GenesisAccount, shared::wasm_config::WasmConfig,
};
use casper_types::auction::EraId;

use super::{chainspec, DeployConfig, Error, HighwayConfig};
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
const DEFAULT_VALIDATOR_SLOTS: u32 = 5;
const DEFAULT_AUCTION_DELAY: u64 = 3;
const DEFAULT_LOCKED_FUNDS_PERIOD: EraId = 15;
/// Round seigniorage rate represented as a fractional number
///
/// Annual issuance: 2%
/// Minimum round exponent: 14
/// Ticks per year: 31536000000
///
/// (1+0.02)^((2^14)/31536000000)-1 is expressed as a fraction below.
const DEFAULT_ROUND_SEIGNIORAGE_RATE: Ratio<u64> = Ratio::new_raw(6414, 623437335209);
const DEFAULT_UNBONDING_DELAY: EraId = 14;
const DEFAULT_WASMLESS_TRANSFER_COST: u64 = 10_000;

#[derive(PartialEq, Eq, Serialize, Deserialize, Debug)]
struct Genesis {
    name: String,
    timestamp: Timestamp,
    validator_slots: u32,
    auction_delay: u64,
    locked_funds_period: EraId,
    protocol_version: Version,
    round_seigniorage_rate: Ratio<u64>,
    unbonding_delay: EraId,
    wasmless_transfer_cost: u64,
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
            validator_slots: DEFAULT_VALIDATOR_SLOTS,
            auction_delay: DEFAULT_AUCTION_DELAY,
            locked_funds_period: DEFAULT_LOCKED_FUNDS_PERIOD,
            protocol_version: Version::from((1, 0, 0)),
            round_seigniorage_rate: DEFAULT_ROUND_SEIGNIORAGE_RATE,
            unbonding_delay: DEFAULT_UNBONDING_DELAY,
            wasmless_transfer_cost: DEFAULT_WASMLESS_TRANSFER_COST,
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
    new_wasm_config: Option<WasmConfig>,
    new_deploy_config: Option<DeployConfig>,
    new_validator_slots: Option<u32>,
}

impl From<&chainspec::UpgradePoint> for UpgradePoint {
    fn from(upgrade_point: &chainspec::UpgradePoint) -> Self {
        UpgradePoint {
            protocol_version: upgrade_point.protocol_version.clone(),
            upgrade_installer_path: Some(External::path(DEFAULT_UPGRADE_INSTALLER_PATH)),
            activation_point: upgrade_point.activation_point,
            new_wasm_config: upgrade_point.new_wasm_config,
            new_deploy_config: upgrade_point.new_deploy_config,
            new_validator_slots: upgrade_point.new_validator_slots,
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
            new_wasm_config: self.new_wasm_config,
            new_deploy_config: self.new_deploy_config,
            new_validator_slots: self.new_validator_slots,
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
    upgrade: Option<Vec<UpgradePoint>>,
    wasm_config: WasmConfig,
}

impl From<&chainspec::Chainspec> for ChainspecConfig {
    fn from(chainspec: &chainspec::Chainspec) -> Self {
        let genesis = Genesis {
            name: chainspec.genesis.name.clone(),
            timestamp: chainspec.genesis.timestamp,
            validator_slots: chainspec.genesis.validator_slots,
            auction_delay: chainspec.genesis.auction_delay,
            locked_funds_period: chainspec.genesis.locked_funds_period,
            protocol_version: chainspec.genesis.protocol_version.clone(),
            round_seigniorage_rate: chainspec.genesis.round_seigniorage_rate,
            unbonding_delay: chainspec.genesis.unbonding_delay,
            wasmless_transfer_cost: chainspec.genesis.wasmless_transfer_cost,
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
        let wasm_config = chainspec.genesis.wasm_config;

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
            upgrade,
            wasm_config,
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
        validator_slots: chainspec.genesis.validator_slots,
        auction_delay: chainspec.genesis.auction_delay,
        locked_funds_period: chainspec.genesis.locked_funds_period,
        round_seigniorage_rate: chainspec.genesis.round_seigniorage_rate,
        unbonding_delay: chainspec.genesis.unbonding_delay,
        wasmless_transfer_cost: chainspec.genesis.wasmless_transfer_cost,
        protocol_version: chainspec.genesis.protocol_version,
        mint_installer_bytes,
        pos_installer_bytes,
        standard_payment_installer_bytes,
        auction_installer_bytes,
        accounts,
        wasm_config: chainspec.wasm_config,
        deploy_config: chainspec.deploys,
        highway_config: chainspec.highway,
    };

    let mut upgrades = vec![];
    for upgrade_point in chainspec.upgrade.unwrap_or_default().into_iter() {
        upgrades.push(upgrade_point.try_into_chainspec_upgrade_point(root)?);
    }

    Ok(chainspec::Chainspec { genesis, upgrades })
}
