mod config;
mod event;
mod tests;

use std::{convert::Infallible, fmt::Debug};

use thiserror::Error;
use tracing::{debug, error, info};

use casper_types::Key;

use crate::{
    components::Component,
    effect::{
        announcements::DeployAcceptorAnnouncement,
        requests::{ContractRuntimeRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects,
    },
    types::{chainspec::DeployConfig, Chainspec, Deploy, DeployValidationFailure, NodeId},
    utils::Source,
    NodeRng,
};

use crate::effect::Responder;
pub(crate) use config::Config;
pub(crate) use event::Event;

#[derive(Debug, Error)]
pub enum Error {
    /// An invalid deploy was received from the client.
    #[error("invalid deploy: {0}")]
    InvalidDeploy(DeployValidationFailure),
    /// An invalid account sent a deploy.
    #[error("invalid account")]
    InvalidAccount,
    /// A deploy was sent from account with insufficient balance.
    #[error("insufficient balance")]
    InsufficientBalance,
}

/// A helper trait constraining `DeployAcceptor` compatible reactor events.
pub(crate) trait ReactorEventT:
    From<Event>
    + From<DeployAcceptorAnnouncement<NodeId>>
    + From<StorageRequest>
    + From<ContractRuntimeRequest>
    + Send
{
}

impl<REv> ReactorEventT for REv where
    REv: From<Event>
        + From<DeployAcceptorAnnouncement<NodeId>>
        + From<StorageRequest>
        + From<ContractRuntimeRequest>
        + Send
{
}

/// The `DeployAcceptor` is the component which handles all new `Deploy`s immediately after they're
/// received by this node, regardless of whether they were provided by a peer or a client.
///
/// It validates a new `Deploy` as far as possible, stores it if valid, then announces the newly-
/// accepted `Deploy`.
#[derive(Debug)]
pub struct DeployAcceptor {
    chain_name: String,
    deploy_config: DeployConfig,
    verify_accounts: bool,
}

impl DeployAcceptor {
    pub(crate) fn new(config: Config, chainspec: &Chainspec) -> Self {
        DeployAcceptor {
            chain_name: chainspec.network_config.name.clone(),
            deploy_config: chainspec.deploy_config,
            verify_accounts: config.verify_accounts(),
        }
    }

    /// Handles receiving a new `Deploy` from a peer or client.
    /// In the case of a peer, there should be no responder and the variant should be `None`
    /// In the case of a client, there should be a responder to communicate the validity of the
    /// deploy and the variant will be `Some`
    fn accept<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    ) -> Effects<Event> {
        let mut cloned_deploy = deploy.clone();
        let mut effects = Effects::new();
        let is_acceptable = cloned_deploy.is_acceptable(&self.chain_name, &self.deploy_config);
        if let Err(error) = is_acceptable {
            // The client has submitted an invalid deploy. Return an error to the RPC component via
            // the responder.
            if let Some(responder) = maybe_responder {
                effects.extend(responder.respond(Err(Error::InvalidDeploy(error))).ignore());
            }
            effects.extend(
                effect_builder
                    .announce_invalid_deploy(deploy, source)
                    .ignore(),
            );
            return effects;
        }

        let account_key = deploy.header().account().to_account_hash().into();

        // Verify account if deploy received from client and node is configured to do so.
        if source.from_client() && self.verify_accounts {
            return effect_builder
                .is_verified_account(account_key)
                .event(move |verified| Event::AccountVerificationResult {
                    deploy,
                    source,
                    account_key,
                    verified,
                    maybe_responder,
                });
        }

        effect_builder
            .immediately()
            .event(move |_| Event::AccountVerificationResult {
                deploy,
                source,
                account_key,
                verified: Some(true),
                maybe_responder,
            })
    }

    fn account_verification<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        account_key: Key,
        verified: Option<bool>,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    ) -> Effects<Event> {
        let mut effects = Effects::new();

        match verified {
            Some(true) => {
                effects.extend(effect_builder.put_deploy_to_storage(deploy.clone()).event(
                    move |is_new| Event::PutToStorageResult {
                        deploy,
                        source,
                        is_new,
                        maybe_responder,
                    },
                ));

                return effects;
            }

            Some(false) => {
                info! {
                    "Received deploy from account {} that does not have minimum balance required", account_key
                };
                // The client has submitted a deploy from an account that does not have minimum
                // balance required. Return an error message to the RPC component via the responder.
                if let Some(responder) = maybe_responder {
                    effects.extend(responder.respond(Err(Error::InsufficientBalance)).ignore());
                }
            }

            None => {
                // The client has submitted an invalid deploy. Return an error message to the RPC
                // component via the responder.
                info! {
                    "Received deploy from invalid account using {}", account_key
                };
                if let Some(responder) = maybe_responder {
                    effects.extend(responder.respond(Err(Error::InvalidAccount)).ignore());
                }
            }
        }

        effects.extend(
            effect_builder
                .announce_invalid_deploy(deploy, source)
                .ignore(),
        );
        effects
    }

    fn handle_put_to_storage<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        is_new: bool,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    ) -> Effects<Event> {
        let mut effects = Effects::new();
        if is_new {
            effects.extend(
                effect_builder
                    .announce_new_deploy_accepted(deploy, source)
                    .ignore(),
            );
        }
        // We can now respond with result of accepting of the deploy
        if let Some(responder) = maybe_responder {
            effects.extend(responder.respond(Ok(())).ignore());
        }
        effects
    }
}

impl<REv: ReactorEventT> Component<REv> for DeployAcceptor {
    type Event = Event;
    type ConstructionError = Infallible;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        debug!(?event, "handling event");
        match event {
            Event::Accept {
                deploy,
                source,
                responder,
            } => self.accept(effect_builder, deploy, source, responder),
            Event::PutToStorageResult {
                deploy,
                source,
                is_new,
                maybe_responder,
            } => {
                self.handle_put_to_storage(effect_builder, deploy, source, is_new, maybe_responder)
            }
            Event::AccountVerificationResult {
                deploy,
                source,
                account_key,
                verified,
                maybe_responder,
            } => self.account_verification(
                effect_builder,
                deploy,
                source,
                account_key,
                verified,
                maybe_responder,
            ),
        }
    }
}
