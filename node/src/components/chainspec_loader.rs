//! Chainspec loader component.
//!
//! The chainspec loader initializes a node by reading information from the chainspec, running
//! initial system contracts and storing the result in the permanent storage. This kind of
//! initialization only happens at genesis.
//!
//! See
//! https://casperlabs.atlassian.net/wiki/spaces/EN/pages/135528449/Genesis+Process+Specification
//! for full details.

mod chainspec;
mod config;
mod error;

use std::fmt::{self, Display, Formatter};

use rand::{CryptoRng, Rng};
use semver::Version;
use tracing::{debug, error, info, trace};

use crate::{
    components::{
        contract_runtime::core::engine_state::{self, genesis::GenesisResult},
        storage::Storage,
        Component,
    },
    crypto::hash::Digest,
    effect::{
        requests::{ContractRuntimeRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects,
    },
};
pub(crate) use chainspec::{DeployConfig, HighwayConfig};
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
    PutToStorage { version: Version },
    /// The result of contract runtime running the genesis process.
    CommitGenesisResult(Result<GenesisResult, engine_state::Error>),
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
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

#[derive(Debug)]
pub(crate) struct ChainspecLoader {
    chainspec: Chainspec,
    // If `Some`, we're finished.  The value of the bool indicates success (true) or not.
    completed_successfully: Option<bool>,
    // If `Some` then genesis process returned a valid post state hash.
    genesis_post_state_hash: Option<Digest>,
}

impl ChainspecLoader {
    pub(crate) fn new<REv>(
        chainspec: Chainspec,
        effect_builder: EffectBuilder<REv>,
    ) -> Result<(Self, Effects<Event>), Error>
    where
        REv: From<Event> + From<StorageRequest<Storage>> + Send,
    {
        let version = chainspec.genesis.protocol_version.clone();
        let effects = effect_builder
            .put_chainspec(chainspec.clone())
            .event(|_| Event::PutToStorage { version });
        Ok((
            ChainspecLoader {
                chainspec,
                completed_successfully: None,
                genesis_post_state_hash: None,
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

    pub(crate) fn genesis_post_state_hash(&self) -> &Option<Digest> {
        &self.genesis_post_state_hash
    }

    pub(crate) fn chainspec(&self) -> &Chainspec {
        &self.chainspec
    }
}

impl<REv, R> Component<REv, R> for ChainspecLoader
where
    REv: From<Event> + From<StorageRequest<Storage>> + From<ContractRuntimeRequest> + Send,
    R: Rng + CryptoRng + ?Sized,
{
    type Event = Event;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut R,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
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
                            info!("successfully committed genesis");
                            trace!(%post_state_hash, ?effect);
                            self.completed_successfully = Some(true);
                            self.genesis_post_state_hash = Some(post_state_hash);
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
