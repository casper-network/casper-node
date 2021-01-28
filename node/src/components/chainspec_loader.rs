//! Chainspec loader component.
//!
//! The chainspec loader initializes a node by reading information from the chainspec, and
//! committing it to the permanent storage.
//!
//! See
//! <https://casperlabs.atlassian.net/wiki/spaces/EN/pages/135528449/Genesis+Process+Specification>
//! for full details.

use std::{
    fmt::{self, Display, Formatter},
    path::{Path, PathBuf},
};

use datasize::DataSize;
use derive_more::From;
use once_cell::sync::Lazy;
use semver::Version;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, trace};

use casper_execution_engine::core::engine_state::{self, genesis::GenesisResult};

#[cfg(test)]
use crate::utils::RESOURCES_PATH;
use crate::{
    components::Component,
    crypto::hash::Digest,
    effect::{
        requests::{ChainspecLoaderRequest, ContractRuntimeRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects,
    },
    rpcs::docs::DocExample,
    types::{chainspec::Error, Chainspec},
    utils::Loadable,
    NodeRng,
};

static CHAINSPEC_INFO: Lazy<ChainspecInfo> = Lazy::new(|| ChainspecInfo {
    name: String::from("casper-example"),
    root_hash: Some(Digest::from([2u8; Digest::LENGTH])),
});

/// `ChainspecHandler` events.
#[derive(Debug, From, Serialize)]
pub enum Event {
    #[from]
    Request(ChainspecLoaderRequest),
    /// The result of the `ChainspecHandler` putting a `Chainspec` to the storage component.
    PutToStorage { version: Version },
    /// The result of contract runtime running the genesis process.
    CommitGenesisResult(#[serde(skip_serializing)] Result<GenesisResult, engine_state::Error>),
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Request(_) => write!(formatter, "chainspec_loader request"),
            Event::PutToStorage { version } => {
                write!(formatter, "put chainspec {} to storage", version)
            }
            Event::CommitGenesisResult(result) => match result {
                Ok(genesis_result) => {
                    write!(formatter, "commit genesis result: {}", genesis_result)
                }
                Err(error) => write!(formatter, "failed to commit genesis: {}", error),
            },
        }
    }
}

#[derive(DataSize, Debug, Serialize, Deserialize, Clone)]
pub struct ChainspecInfo {
    // Name of the chainspec.
    name: String,
    // If `Some` then genesis process returned a valid post state hash.
    root_hash: Option<Digest>,
}

impl ChainspecInfo {
    pub(crate) fn new(name: String, root_hash: Option<Digest>) -> ChainspecInfo {
        ChainspecInfo { name, root_hash }
    }

    pub fn name(&self) -> String {
        self.name.clone()
    }

    pub fn root_hash(&self) -> Option<Digest> {
        self.root_hash
    }
}

impl DocExample for ChainspecInfo {
    fn doc_example() -> &'static Self {
        &*CHAINSPEC_INFO
    }
}

impl From<ChainspecLoader> for ChainspecInfo {
    fn from(chainspec_loader: ChainspecLoader) -> Self {
        ChainspecInfo::new(
            chainspec_loader.chainspec.network_config.name.clone(),
            chainspec_loader.genesis_state_root_hash,
        )
    }
}

#[derive(Clone, DataSize, Debug)]
pub struct ChainspecLoader {
    chainspec: Chainspec,
    /// The path to the folder where all chainspec and upgrade_point files will be stored in
    /// subdirs corresponding to their versions.
    root_dir: PathBuf,
    /// If `Some`, we're finished.  The value of the bool indicates success (true) or not.
    completed_successfully: Option<bool>,
    /// If `Some` then genesis process returned a valid state root hash.
    genesis_state_root_hash: Option<Digest>,
}

impl ChainspecLoader {
    pub(crate) fn new<P, REv>(
        chainspec_dir: P,
        effect_builder: EffectBuilder<REv>,
    ) -> Result<(Self, Effects<Event>), Error>
    where
        P: AsRef<Path>,
        REv: From<Event> + From<StorageRequest> + Send,
    {
        Ok(Self::new_with_chainspec_and_path(
            Chainspec::from_path(&chainspec_dir.as_ref())?,
            chainspec_dir,
            effect_builder,
        ))
    }

    #[cfg(test)]
    pub(crate) fn new_with_chainspec<REv>(
        chainspec: Chainspec,
        effect_builder: EffectBuilder<REv>,
    ) -> (Self, Effects<Event>)
    where
        REv: From<Event> + From<StorageRequest> + Send,
    {
        Self::new_with_chainspec_and_path(chainspec, &RESOURCES_PATH.join("local"), effect_builder)
    }

    fn new_with_chainspec_and_path<P, REv>(
        chainspec: Chainspec,
        chainspec_dir: P,
        effect_builder: EffectBuilder<REv>,
    ) -> (Self, Effects<Event>)
    where
        P: AsRef<Path>,
        REv: From<Event> + From<StorageRequest> + Send,
    {
        chainspec.validate_config();
        let root_dir = chainspec_dir
            .as_ref()
            .parent()
            .unwrap_or_else(|| {
                panic!("chainspec dir must have a parent");
            })
            .to_path_buf();

        let version = chainspec.protocol_config.version.clone();
        let effects = effect_builder
            .put_chainspec(chainspec.clone())
            .event(|_| Event::PutToStorage { version });
        let chainspec_loader = ChainspecLoader {
            chainspec,
            root_dir,
            completed_successfully: None,
            genesis_state_root_hash: None,
        };

        (chainspec_loader, effects)
    }

    pub(crate) fn is_stopped(&self) -> bool {
        self.completed_successfully.is_some()
    }

    pub(crate) fn stopped_successfully(&self) -> bool {
        self.completed_successfully.unwrap_or_default()
    }

    pub(crate) fn genesis_state_root_hash(&self) -> &Option<Digest> {
        &self.genesis_state_root_hash
    }

    pub(crate) fn chainspec(&self) -> &Chainspec {
        &self.chainspec
    }
}

impl<REv> Component<REv> for ChainspecLoader
where
    REv: From<Event> + From<StorageRequest> + From<ContractRuntimeRequest> + Send,
{
    type Event = Event;
    type ConstructionError = Error;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Request(ChainspecLoaderRequest::GetChainspecInfo(req)) => {
                req.respond(self.clone().into()).ignore()
            }
            Event::PutToStorage { version } => {
                debug!("stored chainspec {}", version);
                effect_builder
                    .commit_genesis(self.chainspec.clone())
                    .event(Event::CommitGenesisResult)
            }
            Event::CommitGenesisResult(result) => {
                match result {
                    Ok(genesis_result) => match genesis_result {
                        GenesisResult::RootNotFound
                        | GenesisResult::KeyNotFound(_)
                        | GenesisResult::TypeMismatch(_)
                        | GenesisResult::Serialization(_) => {
                            error!("failed to commit genesis: {}", genesis_result);
                            self.completed_successfully = Some(false);
                        }
                        GenesisResult::Success {
                            post_state_hash,
                            effect,
                        } => {
                            info!("chainspec name {}", self.chainspec.network_config.name);
                            info!("genesis state root hash {}", post_state_hash);
                            trace!(%post_state_hash, ?effect);
                            self.completed_successfully = Some(true);
                            self.genesis_state_root_hash = Some(post_state_hash.into());
                        }
                    },
                    Err(error) => {
                        error!("failed to commit genesis: {}", error);
                        self.completed_successfully = Some(false);
                    }
                }
                Effects::new()
            }
        }
    }
}
