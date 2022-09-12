//! Chainspec loader component.
//!
//! The chainspec loader initializes a node by reading information from the chainspec or an
//! upgrade_point, and committing it to the permanent storage.
//!
//! See
//! <https://casperlabs.atlassian.net/wiki/spaces/EN/pages/135528449/Genesis+Process+Specification>
//! for full details.

// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use std::{path::PathBuf, sync::Arc, time::Duration};

use datasize::DataSize;
use thiserror::Error;
use tracing::{error, trace};

use casper_types::{EraId, ProtocolVersion};

use crate::{
    components::Component,
    effect::{requests::ChainspecLoaderRequest, EffectBuilder, EffectExt, Effects},
    types::{chainspec::ChainspecRawBytes, Chainspec, ChainspecInfo},
    NodeRng,
};

const UPGRADE_CHECK_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error("invalid chainspec")]
    InvalidChainspec,
}

#[derive(Clone, DataSize, Debug)]
pub(crate) struct ChainspecLoader {
    chainspec: Arc<Chainspec>,
    chainspec_raw_bytes: Arc<ChainspecRawBytes>,
}

impl ChainspecLoader {
    pub(crate) fn new(
        chainspec: Arc<Chainspec>,
        chainspec_raw_bytes: Arc<ChainspecRawBytes>,
    ) -> Result<Self, Error> {
        if !chainspec.is_valid() {
            return Err(Error::InvalidChainspec);
        }

        let chainspec_loader = ChainspecLoader {
            chainspec,
            chainspec_raw_bytes,
        };

        Ok(chainspec_loader)
    }

    pub(crate) fn chainspec(&self) -> &Arc<Chainspec> {
        &self.chainspec
    }

    /// Returns the era ID of where we should reset back to.  This means stored blocks in that and
    /// subsequent eras are deleted from storage.
    pub(crate) fn hard_reset_to_start_of_era(&self) -> Option<EraId> {
        self.chainspec
            .protocol_config
            .hard_reset
            .then(|| self.chainspec.protocol_config.activation_point.era_id())
    }

    fn new_chainspec_info(&self) -> ChainspecInfo {
        ChainspecInfo::new(self.chainspec.network_config.name.clone())
    }
}

impl<REv> Component<REv> for ChainspecLoader
where
    REv: From<ChainspecLoaderRequest> + Send,
{
    type Event = ChainspecLoaderRequest;
    type ConstructionError = Error;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        trace!("{}", event);
        match event {
            ChainspecLoaderRequest::GetChainspecInfo(responder) => {
                responder.respond(self.new_chainspec_info()).ignore()
            }
            ChainspecLoaderRequest::GetChainspecRawBytes(responder) => responder
                .respond(Arc::clone(&self.chainspec_raw_bytes))
                .ignore(),
        }
    }
}

fn dir_name_from_version(version: &ProtocolVersion) -> PathBuf {
    PathBuf::from(version.to_string().replace('.', "_"))
}
