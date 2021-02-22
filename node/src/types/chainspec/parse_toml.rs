//! Helper struct and function for parsing a chainspec configuration file into its respective domain
//! object.
//!
//! This is necessary because the `network_config` field of the `Chainspec` struct contains a `Vec`
//! of chainspec accounts, but the chainspec toml file contains a path to a further file which
//! contains the accounts' details, rather than the chainspec file containing the accounts' details
//! itself.

use std::path::Path;

use serde::{Deserialize, Serialize};

use casper_execution_engine::shared::{system_config::SystemConfig, wasm_config::WasmConfig};

use super::{
    accounts_config::AccountsConfig, Chainspec, CoreConfig, DeployConfig, Error, HighwayConfig,
    NetworkConfig, ProtocolConfig,
};
use crate::{
    types::Timestamp,
    utils::{self, Loadable},
};

#[derive(PartialEq, Eq, Serialize, Deserialize, Debug)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
struct TomlNetwork {
    name: String,
    timestamp: Timestamp,
}

/// A chainspec configuration as laid out in the TOML-encoded configuration file.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub(super) struct TomlChainspec {
    protocol: ProtocolConfig,
    network: TomlNetwork,
    core: CoreConfig,
    deploys: DeployConfig,
    highway: HighwayConfig,
    wasm: WasmConfig,
    system_costs: SystemConfig,
}

impl From<&Chainspec> for TomlChainspec {
    fn from(chainspec: &Chainspec) -> Self {
        let protocol = chainspec.protocol_config.clone();
        let network = TomlNetwork {
            name: chainspec.network_config.name.clone(),
            timestamp: chainspec.network_config.timestamp,
        };
        let core = chainspec.core_config;
        let deploys = chainspec.deploy_config;
        let highway = chainspec.highway_config;
        let wasm = chainspec.wasm_config;
        let system_costs = chainspec.system_costs_config;

        TomlChainspec {
            protocol,
            network,
            core,
            deploys,
            highway,
            wasm,
            system_costs,
        }
    }
}

pub(super) fn parse_toml<P: AsRef<Path>>(chainspec_path: P) -> Result<Chainspec, Error> {
    let bytes = utils::read_file(chainspec_path.as_ref()).map_err(Error::LoadChainspec)?;
    let toml_chainspec: TomlChainspec = toml::from_slice(&bytes)?;

    let root = chainspec_path
        .as_ref()
        .parent()
        .unwrap_or_else(|| Path::new(""));

    // accounts.toml must live in the same directory as chainspec.toml.
    let accounts_config = AccountsConfig::from_path(root)?;

    let network_config = NetworkConfig {
        name: toml_chainspec.network.name,
        timestamp: toml_chainspec.network.timestamp,
        accounts_config,
    };

    Ok(Chainspec {
        protocol_config: toml_chainspec.protocol,
        network_config,
        core_config: toml_chainspec.core,
        deploy_config: toml_chainspec.deploys,
        highway_config: toml_chainspec.highway,
        wasm_config: toml_chainspec.wasm,
        system_costs_config: toml_chainspec.system_costs,
    })
}
