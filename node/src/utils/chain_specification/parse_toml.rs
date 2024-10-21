//! Helper struct and function for parsing a chainspec configuration file into its respective domain
//! object.
//!
//! The runtime representation defined by the chainspec object graph is all-inclusive.
//! However, as an implementation detail, the reference implementation splits the data up into
//! multiple topical files.
//!
//! In addition to the mandatory base chainspec file, there is a file containing genesis account
//! definitions for a given network (produced at genesis). This file contains all accounts that will
//! be (or were, historically) created at genesis, their initial balances, initial staking (both
//! validators and delegators). The total initial supply of a new network is a consequence of the
//! sum of the token issued to these accounts. For a test network or small sidechain, the contents
//! of this file might be small but for a full sized network there is quite a lot of data.
//!
//!
//! Further, when protocol version upgrades are put forth they are allowed to have a file containing
//! proposed changes to global state that if accepted will be applied as of the upgrade's block
//! height and onward. This file is optional (more clearly, on an as needed basis only), a given
//! network might not ever have such a file over its lifetime, and the contents of the file can
//! be arbitrarily large as it contains encoded bytes of data. Each such file is directly associated
//! to the specific chainspec file the changes are proposed with; each one is essentially a one off.
//!
//! This capability can and has been used to allow the introduction of new capabilities to the
//! system which require some introduction of value(s) to global state to enable; this is a purely
//! additive / extension type upgrade. However, this capability can also be leveraged as part of a
//! social consensus to make changes to the validator set and / or to assert new values for existing
//! global state entries. In either case, the contents of the file are parseable and verifiable in
//! advance of their acceptance and application to a given network.

use std::{convert::TryFrom, path::Path};

use serde::{Deserialize, Serialize};

use casper_types::{
    bytesrepr::Bytes, file_utils, AccountsConfig, ActivationPoint, Chainspec, ChainspecRawBytes,
    CoreConfig, GlobalStateUpdate, GlobalStateUpdateConfig, HighwayConfig, NetworkConfig,
    ProtocolConfig, ProtocolVersion, StorageCosts, SystemConfig, TransactionConfig, VacancyConfig,
    WasmConfig,
};

use crate::utils::{
    chain_specification::error::{ChainspecAccountsLoadError, Error, GlobalStateUpdateLoadError},
    Loadable,
};

// The names of chainspec related files on disk.
/// The chainspec file name.
pub const CHAINSPEC_FILENAME: &str = "chainspec.toml";
/// The genesis accounts file name.
pub const CHAINSPEC_ACCOUNTS_FILENAME: &str = "accounts.toml";
/// The global state update file name.
pub const CHAINSPEC_GLOBAL_STATE_FILENAME: &str = "global_state.toml";

#[derive(PartialEq, Eq, Serialize, Deserialize, Debug)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
struct TomlNetwork {
    name: String,
    maximum_net_message_size: u32,
}

#[derive(PartialEq, Eq, Serialize, Deserialize, Debug)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
struct TomlProtocol {
    version: ProtocolVersion,
    hard_reset: bool,
    activation_point: ActivationPoint,
}

/// A chainspec configuration as laid out in the TOML-encoded configuration file.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub(super) struct TomlChainspec {
    protocol: TomlProtocol,
    network: TomlNetwork,
    core: CoreConfig,
    transactions: TransactionConfig,
    highway: HighwayConfig,
    wasm: WasmConfig,
    system_costs: SystemConfig,
    vacancy: VacancyConfig,
    storage_costs: StorageCosts,
}

impl From<&Chainspec> for TomlChainspec {
    fn from(chainspec: &Chainspec) -> Self {
        let protocol = TomlProtocol {
            version: chainspec.protocol_config.version,
            hard_reset: chainspec.protocol_config.hard_reset,
            activation_point: chainspec.protocol_config.activation_point,
        };
        let network = TomlNetwork {
            name: chainspec.network_config.name.clone(),
            maximum_net_message_size: chainspec.network_config.maximum_net_message_size,
        };
        let core = chainspec.core_config.clone();
        let transactions = chainspec.transaction_config.clone();
        let highway = chainspec.highway_config;
        let wasm = chainspec.wasm_config;
        let system_costs = chainspec.system_costs_config;
        let vacancy = chainspec.vacancy_config;
        let storage_costs = chainspec.storage_costs;

        TomlChainspec {
            protocol,
            network,
            core,
            transactions,
            highway,
            wasm,
            system_costs,
            vacancy,
            storage_costs,
        }
    }
}

pub(super) fn parse_toml<P: AsRef<Path>>(
    chainspec_path: P,
) -> Result<(Chainspec, ChainspecRawBytes), Error> {
    let chainspec_bytes =
        file_utils::read_file(chainspec_path.as_ref()).map_err(Error::LoadChainspec)?;
    let toml_chainspec: TomlChainspec =
        toml::from_str(std::str::from_utf8(&chainspec_bytes).unwrap())?;

    let root = chainspec_path
        .as_ref()
        .parent()
        .unwrap_or_else(|| Path::new(""));

    // accounts.toml must live in the same directory as chainspec.toml.
    let (accounts_config, maybe_genesis_accounts_bytes) = parse_toml_accounts(root)?;

    let network_config = NetworkConfig {
        name: toml_chainspec.network.name,
        accounts_config,
        maximum_net_message_size: toml_chainspec.network.maximum_net_message_size,
    };

    // global_state_update.toml must live in the same directory as chainspec.toml.
    let (global_state_update, maybe_global_state_bytes) = match parse_toml_global_state(root)? {
        Some((config, bytes)) => (
            Some(
                GlobalStateUpdate::try_from(config)
                    .map_err(GlobalStateUpdateLoadError::DecodingKeyValuePairs)?,
            ),
            Some(bytes),
        ),
        None => (None, None),
    };

    let protocol_config = ProtocolConfig {
        version: toml_chainspec.protocol.version,
        hard_reset: toml_chainspec.protocol.hard_reset,
        activation_point: toml_chainspec.protocol.activation_point,
        global_state_update,
    };

    let chainspec = Chainspec {
        protocol_config,
        network_config,
        core_config: toml_chainspec.core,
        transaction_config: toml_chainspec.transactions,
        highway_config: toml_chainspec.highway,
        wasm_config: toml_chainspec.wasm,
        system_costs_config: toml_chainspec.system_costs,
        vacancy_config: toml_chainspec.vacancy,
        storage_costs: toml_chainspec.storage_costs,
    };
    let chainspec_raw_bytes = ChainspecRawBytes::new(
        Bytes::from(chainspec_bytes),
        maybe_genesis_accounts_bytes,
        maybe_global_state_bytes,
    );

    Ok((chainspec, chainspec_raw_bytes))
}

impl Loadable for (Chainspec, ChainspecRawBytes) {
    type Error = Error;

    fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, Self::Error> {
        parse_toml(path.as_ref().join(CHAINSPEC_FILENAME))
    }
}

/// Returns `Self` and the raw bytes of the file.
///
/// If the file doesn't exist, returns `Ok` with an empty `AccountsConfig` and `None` bytes.
pub(super) fn parse_toml_accounts<P: AsRef<Path>>(
    dir_path: P,
) -> Result<(AccountsConfig, Option<Bytes>), ChainspecAccountsLoadError> {
    let accounts_path = dir_path.as_ref().join(CHAINSPEC_ACCOUNTS_FILENAME);
    if !accounts_path.is_file() {
        let config = AccountsConfig::new(vec![], vec![], vec![]);
        let maybe_bytes = None;
        return Ok((config, maybe_bytes));
    }
    let bytes = file_utils::read_file(accounts_path)?;
    let config: AccountsConfig = toml::from_str(std::str::from_utf8(&bytes).unwrap())?;
    Ok((config, Some(Bytes::from(bytes))))
}

pub(super) fn parse_toml_global_state<P: AsRef<Path>>(
    path: P,
) -> Result<Option<(GlobalStateUpdateConfig, Bytes)>, GlobalStateUpdateLoadError> {
    let update_path = path.as_ref().join(CHAINSPEC_GLOBAL_STATE_FILENAME);
    if !update_path.is_file() {
        return Ok(None);
    }
    let bytes = file_utils::read_file(update_path)?;
    let config = toml::from_str(std::str::from_utf8(&bytes).unwrap())?;
    Ok(Some((config, Bytes::from(bytes))))
}
