mod event;
// mod tests;

use std::fmt::Debug;

use rand::Rng;
use semver::Version;
use tracing::{debug, error, warn};

use crate::{
    components::{
        chainspec_loader::Chainspec,
        storage::{Storage, Value},
        Component,
    },
    effect::{
        announcements::DeployAcceptorAnnouncement, requests::StorageRequest, EffectBuilder,
        EffectExt, Effects,
    },
    small_network::NodeId,
    types::{Deploy, Timestamp},
    utils::Source,
};

pub use event::Event;

/// A helper trait constraining `DeployAcceptor` compatible reactor events.
pub trait ReactorEventT:
    From<Event> + From<DeployAcceptorAnnouncement<NodeId>> + From<StorageRequest<Storage>> + Send
{
}

impl<REv> ReactorEventT for REv where
    REv: From<Event>
        + From<DeployAcceptorAnnouncement<NodeId>>
        + From<StorageRequest<Storage>>
        + Send
{
}

/// The `DeployAcceptor` is the component which handles all new `Deploy`s immediately after they're
/// received by this node, regardless of whether they were provided by a peer or a client.
///
/// It validates a new `Deploy` as far as possible, stores it if valid, then announces the newly-
/// accepted `Deploy`.
#[derive(Debug, Default)]
pub(crate) struct DeployAcceptor {}

impl DeployAcceptor {
    pub(crate) fn new() -> Self {
        Self::default()
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
        effect_builder
            .get_chainspec(chainspec_version.clone())
            .event(move |maybe_chainspec| Event::GetChainspecResult {
                deploy,
                source,
                chainspec_version,
                maybe_chainspec: Box::new(maybe_chainspec),
            })
    }

    fn validate<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        chainspec: Chainspec,
    ) -> Effects<Event> {
        if is_valid(&*deploy, chainspec) {
            let cloned_deploy = deploy.clone();
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

    fn handle_event<R: Rng + ?Sized>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut R,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        debug!(?event, "handling event");
        match event {
            Event::Accept { deploy, source } => self.accept(effect_builder, deploy, source),
            Event::GetChainspecResult {
                deploy,
                source,
                chainspec_version,
                maybe_chainspec,
            } => match *maybe_chainspec {
                Some(chainspec) => self.validate(effect_builder, deploy, source, chainspec),
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

fn is_valid(deploy: &Deploy, chainspec: Chainspec) -> bool {
    if deploy.header().chain_name != chainspec.genesis.name {
        warn!(
            deploy_hash = %deploy.id(),
            deploy_header = %deploy.header(),
            chain_name = %chainspec.genesis.name,
            "deploy ttl excessive"
        );
        return false;
    }

    if deploy.header().dependencies.len()
        > chainspec.genesis.deploy_config.max_dependencies as usize
    {
        warn!(
            deploy_hash = %deploy.id(),
            deploy_header = %deploy.header(),
            max_dependencies = %chainspec.genesis.deploy_config.max_dependencies,
            "deploy ttl excessive"
        );
        return false;
    }

    if deploy.header().ttl_millis as u128 > chainspec.genesis.deploy_config.max_ttl.as_millis() {
        warn!(
            deploy_hash = %deploy.id(),
            deploy_header = %deploy.header(),
            max_ttl = %chainspec.genesis.deploy_config.max_ttl.as_millis(),
            "deploy ttl excessive"
        );
        return false;
    }

    let now = Timestamp::now();
    if now.millis() > deploy.header().expires() {
        warn!(
            deploy_hash = %deploy.id(),
            deploy_header = %deploy.header(),
            %now,
            "deploy expired"
        );
        return false;
    }

    // TODO - check if there is more that can be validated here.

    true
}
