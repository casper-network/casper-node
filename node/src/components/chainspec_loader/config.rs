//! Helper structs used to parse chainspec configuration files into their respective domain objects.

use std::path::Path;

use num_rational::Ratio;
use semver::Version;
use serde::{Deserialize, Serialize};

use casper_execution_engine::{
    core::engine_state::genesis::GenesisAccount,
    shared::{system_config::SystemConfig, wasm_config::WasmConfig},
};
use casper_types::auction::EraId;

use super::{chainspec, DeployConfig, Error, HighwayConfig};
use crate::{
    crypto::hash::Digest,
    types::Timestamp,
    utils::{read_file, External},
};

const DEFAULT_CHAIN_NAME: &str = "casper-devnet";
const DEFAULT_ACCOUNTS_CSV_PATH: &str = "accounts.csv";
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
    accounts_path: External<Vec<GenesisAccount>>,
    state_root_hash: Option<Digest>,
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
            accounts_path: External::path(DEFAULT_ACCOUNTS_CSV_PATH),
            state_root_hash: None,
        }
    }
}

#[derive(PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct UpgradePoint {
    protocol_version: Version,
    activation_point: chainspec::ActivationPoint,
    new_wasm_config: Option<WasmConfig>,
    new_system_config: Option<SystemConfig>,
    new_deploy_config: Option<DeployConfig>,
    new_validator_slots: Option<u32>,
}

impl From<&chainspec::UpgradePoint> for UpgradePoint {
    fn from(upgrade_point: &chainspec::UpgradePoint) -> Self {
        UpgradePoint {
            protocol_version: upgrade_point.protocol_version.clone(),
            activation_point: upgrade_point.activation_point,
            new_wasm_config: upgrade_point.new_wasm_config,
            new_system_config: upgrade_point.new_system_config,
            new_deploy_config: upgrade_point.new_deploy_config,
            new_validator_slots: upgrade_point.new_validator_slots,
        }
    }
}

impl UpgradePoint {
    fn into_chainspec_upgrade_point<P: AsRef<Path>>(self, _root: P) -> chainspec::UpgradePoint {
        chainspec::UpgradePoint {
            activation_point: self.activation_point,
            protocol_version: self.protocol_version,
            new_wasm_config: self.new_wasm_config,
            new_system_config: self.new_system_config,
            new_deploy_config: self.new_deploy_config,
            new_validator_slots: self.new_validator_slots,
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
    upgrade: Option<Vec<UpgradePoint>>,
    wasm_config: WasmConfig,
    system_config: SystemConfig,
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
            accounts_path: External::path(DEFAULT_ACCOUNTS_CSV_PATH),
            state_root_hash: chainspec.genesis.state_root_hash,
        };

        let highway = chainspec.genesis.highway_config.clone();
        let deploys = chainspec.genesis.deploy_config;
        let wasm_config = chainspec.genesis.wasm_config;
        let system_config = chainspec.genesis.system_config;

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
            system_config,
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
        protocol_version: chainspec.genesis.protocol_version,
        accounts,
        wasm_config: chainspec.wasm_config,
        system_config: chainspec.system_config,
        deploy_config: chainspec.deploys,
        highway_config: chainspec.highway,
        state_root_hash: chainspec.genesis.state_root_hash,
    };

    let chainspec_upgrades = chainspec.upgrade.unwrap_or_default();
    let mut upgrades = Vec::with_capacity(chainspec_upgrades.len());
    for upgrade_point in chainspec_upgrades.into_iter() {
        upgrades.push(upgrade_point.into_chainspec_upgrade_point(root));
    }

    Ok(chainspec::Chainspec { genesis, upgrades })
}
