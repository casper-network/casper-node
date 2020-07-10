mod chainspec;
mod config;
mod error;

use std::{
    fmt::{self, Display, Formatter},
    path::PathBuf,
};

use rand::Rng;
use semver::Version;
use tracing::{debug, error, info, trace};

use crate::{
    components::{
        contract_runtime::core::engine_state::{self, genesis::GenesisResult},
        storage::{self, Storage},
        Component,
    },
    effect::{
        requests::{ContractRuntimeRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects,
    },
};
// False positive.
#[allow(unreachable_pub)]
pub use chainspec::{Chainspec, GenesisAccount};
// False positive.
#[allow(unreachable_pub)]
pub use error::Error;

/// `ChainspecHandler` events.
#[derive(Debug)]
pub enum Event {
    /// The result of the `ChainspecHandler` putting a `Chainspec` to the storage component.
    PutToStoreResult {
        version: Version,
        result: storage::Result<()>,
    },
    /// The result of contract runtime running the genesis process.
    CommitGenesisResult(Result<GenesisResult, engine_state::Error>),
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::PutToStoreResult { version, result } => {
                if result.is_ok() {
                    write!(formatter, "put chainspec {} to store", version)
                } else {
                    write!(formatter, "failed to put chainspec {} from store", version)
                }
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

#[derive(Debug)]
pub(crate) struct ChainspecHandler {
    chainspec: Chainspec,
    // If `Some`, we're finished.  The value of the bool indicates success (true) or not.
    completed_successfully: Option<bool>,
}

impl ChainspecHandler {
    pub(crate) fn new<REv>(
        chainspec_config_path: PathBuf,
        effect_builder: EffectBuilder<REv>,
    ) -> Result<(Self, Effects<Event>), Error>
    where
        REv: From<Event> + From<StorageRequest<Storage>> + Send,
    {
        let chainspec = Chainspec::from_toml(chainspec_config_path)?;
        let version = chainspec.genesis.protocol_version.clone();
        let effects = effect_builder
            .put_chainspec(chainspec.clone())
            .event(|result| Event::PutToStoreResult { version, result });
        Ok((
            ChainspecHandler {
                chainspec,
                completed_successfully: None,
            },
            effects,
        ))
    }

    pub(crate) fn is_stopped(&self) -> bool {
        self.completed_successfully.is_some()
    }

    pub(crate) fn stopped_successfully(&self) -> bool {
        self.completed_successfully.unwrap_or_default()
    }
}

impl<REv> Component<REv> for ChainspecHandler
where
    REv: From<Event> + From<StorageRequest<Storage>> + From<ContractRuntimeRequest> + Send,
{
    type Event = Event;

    fn handle_event<R: Rng + ?Sized>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut R,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::PutToStoreResult { version, result } => match result {
                Ok(()) => {
                    debug!("stored chainspec {}", version);
                    effect_builder
                        .commit_genesis(self.chainspec.clone())
                        .event(Event::CommitGenesisResult)
                }
                Err(error) => {
                    error!("failed to store chainspec {}: {}", version, error);
                    self.completed_successfully = Some(false);
                    Effects::new()
                }
            },
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
                            info!("successfully committed genesis");
                            trace!(%post_state_hash, ?effect);
                            self.completed_successfully = Some(true);
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
