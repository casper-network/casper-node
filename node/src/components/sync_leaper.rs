//! The Sync Leaper
mod error;
mod event;
mod leap_activity;
mod leap_state;
mod metrics;
#[cfg(test)]
mod tests;

use std::{sync::Arc, time::Instant};

use datasize::DataSize;
use prometheus::Registry;
use thiserror::Error;
use tracing::{debug, error, info, warn};

use casper_types::Chainspec;

use crate::{
    components::{
        fetcher::{self, FetchResult, FetchedData},
        Component,
    },
    effect::{requests::FetcherRequest, EffectBuilder, EffectExt, Effects},
    types::{
        sync_leap_validation_metadata::SyncLeapValidationMetaData, NodeId, SyncLeap,
        SyncLeapIdentifier,
    },
    NodeRng,
};
pub(crate) use error::LeapActivityError;
pub(crate) use event::Event;
pub(crate) use leap_state::LeapState;

use metrics::Metrics;

use self::leap_activity::LeapActivity;

const COMPONENT_NAME: &str = "sync_leaper";

#[derive(Clone, Debug, DataSize, Eq, PartialEq)]
pub(crate) enum PeerState {
    RequestSent,
    Rejected,
    CouldntFetch,
    Fetched(Box<SyncLeap>),
}

#[derive(Debug)]
enum RegisterLeapAttemptOutcome {
    DoNothing,
    FetchSyncLeapFromPeers(Vec<NodeId>),
}

#[derive(Debug, Error)]
enum Error {
    #[error("fetched a sync leap from storage - {0}")]
    FetchedSyncLeapFromStorage(SyncLeapIdentifier),
    #[error("received a sync leap response while no requests were in progress - {0}")]
    UnexpectedSyncLeapResponse(SyncLeapIdentifier),
    #[error("block hash in the response '{actual}' doesn't match the one requested '{expected}'")]
    SyncLeapIdentifierMismatch {
        expected: SyncLeapIdentifier,
        actual: SyncLeapIdentifier,
    },
    #[error(
        "received a sync leap response from an unknown peer - {peer} - {sync_leap_identifier}"
    )]
    ResponseFromUnknownPeer {
        peer: NodeId,
        sync_leap_identifier: SyncLeapIdentifier,
    },
}

#[derive(Debug, DataSize)]
pub(crate) struct SyncLeaper {
    leap_activity: Option<LeapActivity>,
    chainspec: Arc<Chainspec>,
    #[data_size(skip)]
    metrics: Metrics,
}

impl SyncLeaper {
    pub(crate) fn new(
        chainspec: Arc<Chainspec>,
        registry: &Registry,
    ) -> Result<Self, prometheus::Error> {
        Ok(SyncLeaper {
            leap_activity: None,
            chainspec,
            metrics: Metrics::new(registry)?,
        })
    }

    /// Returns whether a sync leap is ongoing or completed and its state if so.
    ///
    /// If a sync leap has been completed, successfully or not, the results are returned and the
    /// attempt is removed, effectively making the component idle.
    pub(crate) fn leap_status(&mut self) -> LeapState {
        match &self.leap_activity {
            None => LeapState::Idle,
            Some(activity) => {
                let result = activity.status();
                if result.active() == false {
                    match result {
                        LeapState::Received { .. } | LeapState::Failed { .. } => {
                            self.metrics
                                .sync_leap_duration
                                .observe(activity.leap_start().elapsed().as_secs_f64());
                        }
                        LeapState::Idle | LeapState::Awaiting { .. } => {
                            // should be unreachable
                            error!(status = %result, ?activity, "sync leaper has inconsistent status");
                        }
                    }
                    self.leap_activity = None;
                }
                result
            }
        }
    }

    /// Causes any ongoing sync leap attempt to be abandoned, i.e. results gathered so far are
    /// dropped and responses received later for this attempt are ignored.
    pub(crate) fn purge(&mut self) {
        if let Some(activity) = self.leap_activity.take() {
            debug!(identifier = %activity.sync_leap_identifier(), "purging sync leap");
        }
    }

    #[cfg_attr(doc, aquamarine::aquamarine)]
    /// ```mermaid
    /// flowchart TD
    ///     style Start fill:#66ccff,stroke:#333,stroke-width:4px
    ///     style End fill:#66ccff,stroke:#333,stroke-width:4px
    ///
    ///     title[SyncLeap process - AttemptLeap]
    ///     title---Start
    ///     style title fill:#FFF,stroke:#FFF
    ///     linkStyle 0 stroke-width:0;
    ///
    ///     Start --> A{have at least<br>one peer?}
    ///     A -->|Yes| B{is other sync<br>leap in progress?}
    ///     A -->|No| End
    ///     B -->|Yes| C{do sync leap<br>identifiers match?}
    ///     C -->|No| End
    ///     C -->|Yes| D[fetch SyncLeap from potentially<br>newly learned peers]
    ///     B -->|No| G[fetch SyncLeap<br>from all peers]
    ///     G --> E
    ///     D --> E[SyncLeap arrives]
    ///     E --> F[SyncLeap is stored]
    ///     F --> End
    /// ```
    fn register_leap_attempt(
        &mut self,
        sync_leap_identifier: SyncLeapIdentifier,
        peers_to_ask: Vec<NodeId>,
    ) -> RegisterLeapAttemptOutcome {
        info!(%sync_leap_identifier, "registering leap attempt");
        if peers_to_ask.is_empty() {
            error!("tried to start fetching a sync leap without peers to ask");
            return RegisterLeapAttemptOutcome::DoNothing;
        }
        if let Some(leap_activity) = self.leap_activity.as_mut() {
            if leap_activity.sync_leap_identifier() != &sync_leap_identifier {
                error!(
                    current_sync_leap_identifier = %leap_activity.sync_leap_identifier(),
                    requested_sync_leap_identifier = %sync_leap_identifier,
                    "tried to start fetching a sync leap for a different sync_leap_identifier"
                );
                return RegisterLeapAttemptOutcome::DoNothing;
            }

            let peers_not_asked_yet: Vec<_> = peers_to_ask
                .iter()
                .filter_map(|peer| leap_activity.register_peer(*peer))
                .collect();

            return if peers_not_asked_yet.is_empty() {
                debug!(%sync_leap_identifier, "peers_not_asked_yet.is_empty()");
                RegisterLeapAttemptOutcome::DoNothing
            } else {
                debug!(%sync_leap_identifier, "fetching sync leap from {} peers not asked yet", peers_not_asked_yet.len());
                RegisterLeapAttemptOutcome::FetchSyncLeapFromPeers(peers_not_asked_yet)
            };
        }

        debug!(%sync_leap_identifier, "fetching sync leap from {} peers", peers_to_ask.len());
        self.leap_activity = Some(LeapActivity::new(
            sync_leap_identifier,
            peers_to_ask
                .iter()
                .map(|peer| (*peer, PeerState::RequestSent))
                .collect(),
            Instant::now(),
        ));
        RegisterLeapAttemptOutcome::FetchSyncLeapFromPeers(peers_to_ask)
    }

    fn fetch_received(
        &mut self,
        sync_leap_identifier: SyncLeapIdentifier,
        fetch_result: FetchResult<SyncLeap>,
    ) -> Result<(), Error> {
        let leap_activity = match &mut self.leap_activity {
            Some(leap_activity) => leap_activity,
            None => {
                return Err(Error::UnexpectedSyncLeapResponse(sync_leap_identifier));
            }
        };

        if leap_activity.sync_leap_identifier() != &sync_leap_identifier {
            return Err(Error::SyncLeapIdentifierMismatch {
                actual: sync_leap_identifier,
                expected: *leap_activity.sync_leap_identifier(),
            });
        }

        match fetch_result {
            Ok(FetchedData::FromStorage { .. }) => {
                Err(Error::FetchedSyncLeapFromStorage(sync_leap_identifier))
            }
            Ok(FetchedData::FromPeer { item, peer, .. }) => {
                let peer_state = match leap_activity.peers_mut().get_mut(&peer) {
                    Some(state) => state,
                    None => {
                        return Err(Error::ResponseFromUnknownPeer {
                            peer,
                            sync_leap_identifier,
                        });
                    }
                };
                *peer_state = PeerState::Fetched(Box::new(*item));
                self.metrics.sync_leap_fetched_from_peer.inc();
                Ok(())
            }
            Err(fetcher::Error::Rejected { peer, .. }) => {
                let peer_state = match leap_activity.peers_mut().get_mut(&peer) {
                    Some(state) => state,
                    None => {
                        return Err(Error::ResponseFromUnknownPeer {
                            peer,
                            sync_leap_identifier,
                        });
                    }
                };
                info!(%peer, %sync_leap_identifier, "peer rejected our request for a sync leap");
                *peer_state = PeerState::Rejected;
                self.metrics.sync_leap_rejected_by_peer.inc();
                Ok(())
            }
            Err(error) => {
                let peer = error.peer();
                info!(?error, %peer, %sync_leap_identifier, "failed to fetch a sync leap from peer");
                let peer_state = match leap_activity.peers_mut().get_mut(peer) {
                    Some(state) => state,
                    None => {
                        return Err(Error::ResponseFromUnknownPeer {
                            peer: *peer,
                            sync_leap_identifier,
                        });
                    }
                };
                *peer_state = PeerState::CouldntFetch;
                self.metrics.sync_leap_cant_fetch.inc();
                Ok(())
            }
        }
    }
}

impl<REv> Component<REv> for SyncLeaper
where
    REv: From<FetcherRequest<SyncLeap>> + Send,
{
    type Event = Event;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::AttemptLeap {
                sync_leap_identifier,
                peers_to_ask,
            } => match self.register_leap_attempt(sync_leap_identifier, peers_to_ask) {
                RegisterLeapAttemptOutcome::DoNothing => Effects::new(),
                RegisterLeapAttemptOutcome::FetchSyncLeapFromPeers(peers) => {
                    let mut effects = Effects::new();
                    peers.into_iter().for_each(|peer| {
                        effects.extend(
                            effect_builder
                                .fetch::<SyncLeap>(
                                    sync_leap_identifier,
                                    peer,
                                    Box::new(SyncLeapValidationMetaData::from_chainspec(
                                        self.chainspec.as_ref(),
                                    )),
                                )
                                .event(move |fetch_result| Event::FetchedSyncLeapFromPeer {
                                    sync_leap_identifier,
                                    fetch_result,
                                }),
                        )
                    });
                    effects
                }
            },
            Event::FetchedSyncLeapFromPeer {
                sync_leap_identifier,
                fetch_result,
            } => {
                // Log potential error with proper severity and continue processing.
                if let Err(error) = self.fetch_received(sync_leap_identifier, fetch_result) {
                    match error {
                        Error::FetchedSyncLeapFromStorage(_) => error!(%error),
                        Error::UnexpectedSyncLeapResponse(_)
                        | Error::SyncLeapIdentifierMismatch { .. }
                        | Error::ResponseFromUnknownPeer { .. } => warn!(%error),
                    }
                }
                Effects::new()
            }
        }
    }

    fn name(&self) -> &str {
        COMPONENT_NAME
    }
}

#[cfg(test)]
impl SyncLeaper {
    fn peers(&self) -> Option<Vec<(NodeId, PeerState)>> {
        self.leap_activity
            .as_ref()
            .and_then(|leap_activity| {
                let peers = leap_activity.peers();
                if leap_activity.peers().is_empty() {
                    None
                } else {
                    Some(peers.clone())
                }
            })
            .map(|peers| peers.into_iter().collect::<Vec<_>>())
    }
}
