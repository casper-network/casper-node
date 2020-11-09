mod event;
// mod tests;

use std::{collections::HashMap, convert::Infallible, fmt::Debug};

use semver::Version;
use tracing::{debug, error, warn};

use crate::{
    components::{chainspec_loader::Chainspec, Component},
    effect::{
        announcements::DeployAcceptorAnnouncement, requests::StorageRequest, EffectBuilder,
        EffectExt, Effects,
    },
    small_network::NodeId,
    types::{CryptoRngCore, Deploy},
    utils::Source,
};

pub use event::Event;

use super::chainspec_loader::DeployConfig;

/// A helper trait constraining `DeployAcceptor` compatible reactor events.
pub trait ReactorEventT:
    From<Event> + From<DeployAcceptorAnnouncement<NodeId>> + From<StorageRequest> + Send
{
}

impl<REv> ReactorEventT for REv where
    REv: From<Event> + From<DeployAcceptorAnnouncement<NodeId>> + From<StorageRequest> + Send
{
}

#[derive(Debug, Clone)]
pub struct DeployAcceptorConfig {
    chain_name: String,
    deploy_config: DeployConfig,
}

impl From<Chainspec> for DeployAcceptorConfig {
    fn from(c: Chainspec) -> Self {
        DeployAcceptorConfig {
            chain_name: c.genesis.name,
            deploy_config: c.genesis.deploy_config,
        }
    }
}

/// The `DeployAcceptor` is the component which handles all new `Deploy`s immediately after they're
/// received by this node, regardless of whether they were provided by a peer or a client.
///
/// It validates a new `Deploy` as far as possible, stores it if valid, then announces the newly-
/// accepted `Deploy`.
#[derive(Debug)]
pub(crate) struct DeployAcceptor {
    cached_deploy_configs: HashMap<Version, DeployAcceptorConfig>,
}

impl DeployAcceptor {
    pub(crate) fn new() -> Self {
        DeployAcceptor {
            cached_deploy_configs: HashMap::new(),
        }
    }

    /// Handles receiving a new `Deploy` from a peer or client.
    fn accept<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        deploy: Box<Deploy>,
        source: Source<NodeId>,
    ) -> Effects<Event> {
        // TODO - where to get version from?
        let chainspec_version = Version::new(1, 0, 0);
        let cached_config = self.cached_deploy_configs.get(&chainspec_version).cloned();
        match cached_config {
            Some(genesis_config) => {
                effect_builder
                    .immediately()
                    .event(move |_| Event::GetChainspecResult {
                        deploy,
                        source,
                        chainspec_version,
                        maybe_deploy_config: Box::new(Some(genesis_config)),
                    })
            }
            None => effect_builder
                .get_chainspec(chainspec_version.clone())
                .event(move |maybe_chainspec| Event::GetChainspecResult {
                    deploy,
                    source,
                    chainspec_version,
                    maybe_deploy_config: Box::new(maybe_chainspec.map(|c| (*c).clone().into())),
                }),
        }
    }

    fn validate<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        deploy_config: DeployAcceptorConfig,
    ) -> Effects<Event> {
        let mut cloned_deploy = deploy.clone();
        if is_valid(&mut cloned_deploy, deploy_config) {
            effect_builder
                .put_deploy_to_storage(cloned_deploy)
                .event(move |is_new| Event::PutToStorageResult {
                    deploy,
                    source,
                    is_new,
                })
        } else {
            effect_builder
                .announce_invalid_deploy(deploy, source)
                .ignore()
        }
    }

    fn failed_to_get_chainspec(
        &self,
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        chainspec_version: Version,
    ) -> Effects<Event> {
        error!(%deploy, %source, %chainspec_version, "failed to get chainspec");
        Effects::new()
    }

    fn handle_put_to_storage<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        is_new: bool,
    ) -> Effects<Event> {
        if is_new {
            return effect_builder
                .announce_new_deploy_accepted(deploy, source)
                .ignore();
        }
        Effects::new()
    }
}

impl<REv: ReactorEventT> Component<REv> for DeployAcceptor {
    type Event = Event;
    type ConstructionError = Infallible;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut dyn CryptoRngCore,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        debug!(?event, "handling event");
        match event {
            Event::Accept { deploy, source } => self.accept(effect_builder, deploy, source),
            Event::GetChainspecResult {
                deploy,
                source,
                chainspec_version,
                maybe_deploy_config,
            } => match *maybe_deploy_config {
                Some(deploy_config) => {
                    // Update chainspec cache.
                    self.cached_deploy_configs
                        .insert(chainspec_version, deploy_config.clone());
                    self.validate(effect_builder, deploy, source, deploy_config)
                }
                None => self.failed_to_get_chainspec(deploy, source, chainspec_version),
            },
            Event::PutToStorageResult {
                deploy,
                source,
                is_new,
            } => self.handle_put_to_storage(effect_builder, deploy, source, is_new),
        }
    }
}

fn is_valid(deploy: &mut Deploy, config: DeployAcceptorConfig) -> bool {
    if deploy.header().chain_name() != config.chain_name {
        warn!(
            deploy_hash = %deploy.id(),
            deploy_header = %deploy.header(),
            chain_name = %config.chain_name,
            "invalid chain identifier"
        );
        return false;
    }

    if deploy.header().dependencies().len() > config.deploy_config.max_dependencies as usize {
        warn!(
            deploy_hash = %deploy.id(),
            deploy_header = %deploy.header(),
            max_dependencies = %config.deploy_config.max_dependencies,
            "deploy dependency ceiling exceeded"
        );
        return false;
    }

    if deploy.header().ttl() > config.deploy_config.max_ttl {
        warn!(
            deploy_hash = %deploy.id(),
            deploy_header = %deploy.header(),
            max_ttl = %config.deploy_config.max_ttl,
            "deploy ttl excessive"
        );
        return false;
    }

    // TODO - check if there is more that can be validated here.

    deploy.is_valid()
}
