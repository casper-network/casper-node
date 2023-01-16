mod config;
#[cfg(test)]
mod error;
mod event;
mod gossip_table;
mod message;
mod metrics;
mod tests;

use std::{
    collections::HashSet,
    fmt::{self, Debug, Formatter},
    sync::Arc,
    time::Duration,
};

use datasize::DataSize;
use prometheus::Registry;
use tracing::{debug, error, trace, warn};

use crate::{
    components::Component,
    effect::{
        announcements::GossiperAnnouncement,
        incoming::GossiperIncoming,
        requests::{BeginGossipRequest, NetworkRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects, GossipTarget,
    },
    types::{
        Block, BlockHash, Deploy, DeployId, FinalitySignature, FinalitySignatureId, GossiperItem,
        Item, NodeId,
    },
    utils::Source,
    NodeRng,
};
pub(crate) use config::Config;
pub(crate) use event::Event;
use gossip_table::{GossipAction, GossipTable};
pub(crate) use message::Message;
use metrics::Metrics;

const COMPONENT_NAME: &str = "gossiper";

/// A helper trait whose bounds represent the requirements for a reactor event that `Gossiper` can
/// work with.
pub(crate) trait ReactorEventT<T>:
    From<Event<T>>
    + From<NetworkRequest<Message<T>>>
    + From<StorageRequest>
    + From<GossiperAnnouncement<T>>
    + Send
    + 'static
where
    T: GossiperItem + 'static,
    <T as Item>::Id: 'static,
{
}

impl<REv, T> ReactorEventT<T> for REv
where
    T: GossiperItem + 'static,
    <T as Item>::Id: 'static,
    REv: From<Event<T>>
        + From<NetworkRequest<Message<T>>>
        + From<StorageRequest>
        + From<GossiperAnnouncement<T>>
        + Send
        + 'static,
{
}

/// This function can be passed in to `Gossiper::new()` as the `get_from_holder` arg when
/// constructing a `Gossiper<Deploy>`.
pub(crate) fn get_deploy_from_storage<T: GossiperItem + 'static, REv: ReactorEventT<T>>(
    effect_builder: EffectBuilder<REv>,
    item_id: DeployId,
    sender: NodeId,
) -> Effects<Event<Deploy>> {
    effect_builder
        .get_stored_deploy(item_id)
        .event(move |result| Event::GetFromHolderResult {
            item_id,
            requester: sender,
            result: Box::new(
                result.ok_or_else(|| format!("failed to get {} from storage", item_id)),
            ),
        })
}

pub(crate) fn get_finality_signature_from_storage<
    T: GossiperItem + 'static,
    REv: ReactorEventT<T>,
>(
    effect_builder: EffectBuilder<REv>,
    item_id: FinalitySignatureId,
    requester: NodeId,
) -> Effects<Event<FinalitySignature>> {
    effect_builder
        .get_finality_signature_from_storage(item_id.clone())
        .event(move |results| {
            let result = results.ok_or_else(|| String::from("finality signature not found"));
            Event::GetFromHolderResult {
                item_id,
                requester,
                result: Box::new(result),
            }
        })
}

/// This function can be passed in to `Gossiper::new()` as the `get_from_holder` arg when
/// constructing a `Gossiper<Block>`.
pub(crate) fn get_block_from_storage<T: GossiperItem + 'static, REv: ReactorEventT<T>>(
    effect_builder: EffectBuilder<REv>,
    block_hash: BlockHash,
    sender: NodeId,
) -> Effects<Event<Block>> {
    effect_builder
        .get_block_from_storage(block_hash)
        .event(move |results| {
            let result = results.ok_or_else(|| String::from("block not found"));
            Event::GetFromHolderResult {
                item_id: block_hash,
                requester: sender,
                result: Box::new(result),
            }
        })
}

/// The component which gossips to peers and handles incoming gossip messages from peers.
#[allow(clippy::type_complexity)]
#[derive(DataSize)]
pub(crate) struct Gossiper<T, REv>
where
    T: GossiperItem + 'static,
    REv: ReactorEventT<T>,
{
    table: GossipTable<T::Id>,
    gossip_timeout: Duration,
    get_from_peer_timeout: Duration,
    #[data_size(skip)] // Not well supported by datasize.
    get_from_holder:
        Box<dyn Fn(EffectBuilder<REv>, T::Id, NodeId) -> Effects<Event<T>> + Send + 'static>,
    #[data_size(skip)]
    metrics: Metrics,
}

impl<T: GossiperItem + 'static, REv: ReactorEventT<T>> Gossiper<T, REv> {
    /// Constructs a new gossiper component for use where `T::ID_IS_COMPLETE_ITEM == false`, i.e.
    /// where the gossip messages themselves don't contain the actual data being gossiped, they
    /// contain just the identifiers.
    ///
    /// `get_from_holder` is called by the gossiper when handling either a `Message::GossipResponse`
    /// where the sender indicates it needs the full item, or a `Message::GetRequest`.
    ///
    /// For an example of how `get_from_holder` should be implemented, see
    /// `gossiper::get_deploy_from_store()` which is used by `Gossiper<Deploy>`.
    ///
    /// Must be supplied with a name, which should be a snake-case identifier to disambiguate the
    /// specific gossiper from other potentially present gossipers.
    pub(crate) fn new_for_partial_items(
        name: &str,
        config: Config,
        get_from_holder: impl Fn(EffectBuilder<REv>, T::Id, NodeId) -> Effects<Event<T>>
            + Send
            + 'static,
        registry: &Registry,
    ) -> Result<Self, prometheus::Error> {
        assert!(
            !T::ID_IS_COMPLETE_ITEM,
            "this should only be called for types where T::ID_IS_COMPLETE_ITEM is false"
        );
        Ok(Gossiper {
            table: GossipTable::new(config),
            gossip_timeout: config.gossip_request_timeout().into(),
            get_from_peer_timeout: config.get_remainder_timeout().into(),
            get_from_holder: Box::new(get_from_holder),
            metrics: Metrics::new(name, registry)?,
        })
    }

    /// Constructs a new gossiper component for use where `T::ID_IS_COMPLETE_ITEM == true`, i.e.
    /// where the gossip messages themselves contain the actual data being gossiped.
    ///
    /// Must be supplied with a name, which should be a snake-case identifier to disambiguate the
    /// specific gossiper from other potentially present gossipers.
    pub(crate) fn new_for_complete_items(
        name: &str,
        config: Config,
        registry: &Registry,
    ) -> Result<Self, prometheus::Error> {
        assert!(
            T::ID_IS_COMPLETE_ITEM,
            "this should only be called for types where T::ID_IS_COMPLETE_ITEM is true"
        );
        Ok(Gossiper {
            table: GossipTable::new(config),
            gossip_timeout: config.gossip_request_timeout().into(),
            get_from_peer_timeout: config.get_remainder_timeout().into(),
            get_from_holder: Box::new(|_, item, _| {
                panic!("gossiper should never try to get {}", item)
            }),
            metrics: Metrics::new(name, registry)?,
        })
    }

    /// Handles a new item received from a peer or client for which we should begin gossiping.
    ///
    /// Note that this doesn't include items gossiped to us; those are handled in `handle_gossip()`.
    fn handle_item_received(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        item_id: T::Id,
        source: Source,
    ) -> Effects<Event<T>> {
        debug!(item=%item_id, %source, "received new gossip item");
        match self.table.new_complete_data(&item_id, source.node_id()) {
            GossipAction::ShouldGossip(should_gossip) => {
                self.metrics.items_received.inc();
                self.gossip(
                    effect_builder,
                    item_id,
                    should_gossip.target,
                    should_gossip.count,
                    should_gossip.exclude_peers,
                )
            }
            GossipAction::Noop => Effects::new(),
            GossipAction::AnnounceFinished => {
                effect_builder.announce_finished_gossiping(item_id).ignore()
            }
            GossipAction::GetRemainder { .. } | GossipAction::AwaitingRemainder => {
                error!("can't be waiting for remainder since we hold the complete data");
                Effects::new()
            }
        }
    }

    /// Gossips the given item ID to `count` random peers excluding the indicated ones.
    fn gossip(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        item_id: T::Id,
        gossip_target: GossipTarget,
        count: usize,
        exclude_peers: HashSet<NodeId>,
    ) -> Effects<Event<T>> {
        let message = Message::Gossip(item_id.clone());
        effect_builder
            .gossip_message(message, gossip_target, count, exclude_peers)
            .event(move |peers| Event::GossipedTo {
                item_id,
                requested_count: count,
                peers,
            })
    }

    /// Handles the response from the network component detailing which peers it gossiped to.
    fn gossiped_to(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        item_id: T::Id,
        requested_count: usize,
        peers: HashSet<NodeId>,
    ) -> Effects<Event<T>> {
        self.metrics.times_gossiped.inc_by(peers.len() as u64);
        // We don't have any peers to gossip to, so pause the process, which will eventually result
        // in the entry being removed.
        if peers.is_empty() {
            self.metrics.times_ran_out_of_peers.inc();
        }

        // We didn't gossip to as many peers as was requested.  Reduce the table entry's in-flight
        // count.
        let mut effects = Effects::new();
        if peers.len() < requested_count
            && self
                .table
                .reduce_in_flight_count(&item_id, requested_count - peers.len())
        {
            effects.extend(
                effect_builder
                    .announce_finished_gossiping(item_id.clone())
                    .ignore(),
            );
        }

        // Remember which peers we *tried* to infect.
        self.table
            .register_infection_attempt(&item_id, peers.iter());

        // Set timeouts to check later that the specified peers all responded.
        for peer in peers {
            let item_id = item_id.clone();
            effects.extend(
                effect_builder
                    .set_timeout(self.gossip_timeout)
                    .event(move |_| Event::CheckGossipTimeout { item_id, peer }),
            )
        }

        effects
    }

    /// Checks that the given peer has responded to a previous gossip request we sent it.
    fn check_gossip_timeout(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        item_id: T::Id,
        peer: NodeId,
    ) -> Effects<Event<T>> {
        match self.table.check_timeout(&item_id, peer) {
            GossipAction::ShouldGossip(should_gossip) => self.gossip(
                effect_builder,
                item_id,
                should_gossip.target,
                should_gossip.count,
                should_gossip.exclude_peers,
            ),
            GossipAction::Noop => Effects::new(),
            GossipAction::AnnounceFinished => {
                effect_builder.announce_finished_gossiping(item_id).ignore()
            }
            GossipAction::GetRemainder { .. } | GossipAction::AwaitingRemainder => {
                warn!(
                    "can't have gossiped if we don't hold the complete data - likely the timeout \
                    check was very delayed due to busy reactor"
                );
                Effects::new()
            }
        }
    }

    /// Checks that the given peer has responded to a previous gossip response or `GetRequest` we
    /// sent it indicating we wanted to get the full item from it.
    fn check_get_from_peer_timeout(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        item_id: T::Id,
        peer: NodeId,
    ) -> Effects<Event<T>> {
        match self.table.remove_holder_if_unresponsive(&item_id, peer) {
            GossipAction::ShouldGossip(should_gossip) => self.gossip(
                effect_builder,
                item_id,
                should_gossip.target,
                should_gossip.count,
                should_gossip.exclude_peers,
            ),

            GossipAction::GetRemainder { holder } => {
                // The previous peer failed to provide the item, so we still need to get it.  Send
                // a `GetItem` to a different holder and set a timeout to check we got the response.
                let request = Message::GetItem(item_id.clone());
                let mut effects = effect_builder.send_message(holder, request).ignore();
                effects.extend(
                    effect_builder
                        .set_timeout(self.get_from_peer_timeout)
                        .event(move |_| Event::CheckGetFromPeerTimeout {
                            item_id,
                            peer: holder,
                        }),
                );
                effects
            }

            GossipAction::AnnounceFinished => {
                effect_builder.announce_finished_gossiping(item_id).ignore()
            }

            GossipAction::Noop | GossipAction::AwaitingRemainder => Effects::new(),
        }
    }

    /// Handles an incoming gossip request from a peer on the network.
    fn handle_gossip(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        item_id: T::Id,
        sender: NodeId,
    ) -> Effects<Event<T>> {
        let action = if T::ID_IS_COMPLETE_ITEM {
            self.table.new_complete_data(&item_id, Some(sender))
        } else {
            self.table.new_partial_data(&item_id, sender)
        };
        let mut effects = match action {
            GossipAction::ShouldGossip(should_gossip) => {
                debug!(item=%item_id, %sender, %should_gossip, "received gossip request");
                self.metrics.items_received.inc();
                // Gossip the item ID.
                let mut effects = self.gossip(
                    effect_builder,
                    item_id.clone(),
                    should_gossip.target,
                    should_gossip.count,
                    should_gossip.exclude_peers,
                );

                // If this is a new complete item to us, announce it.
                if T::ID_IS_COMPLETE_ITEM && !should_gossip.is_already_held {
                    debug!(item=%item_id, "announcing new complete gossip item received");
                    effects.extend(
                        effect_builder
                            .announce_complete_item_received_via_gossip(item_id.clone())
                            .ignore(),
                    );
                }

                // Send a response to the sender indicating whether we already hold the item.
                let reply = Message::GossipResponse {
                    item_id: item_id.clone(),
                    is_already_held: should_gossip.is_already_held,
                };
                effects.extend(effect_builder.send_message(sender, reply).ignore());
                effects
            }
            GossipAction::GetRemainder { .. } => {
                debug!(item=%item_id, %sender, %action, "received gossip request");
                self.metrics.items_received.inc();
                // Send a response to the sender indicating we want the full item from them, and set
                // a timeout for this response.
                let reply = Message::GossipResponse {
                    item_id: item_id.clone(),
                    is_already_held: false,
                };
                let mut effects = effect_builder.send_message(sender, reply).ignore();
                let item_id_clone = item_id.clone();
                effects.extend(
                    effect_builder
                        .set_timeout(self.get_from_peer_timeout)
                        .event(move |_| Event::CheckGetFromPeerTimeout {
                            item_id: item_id_clone,
                            peer: sender,
                        }),
                );
                effects
            }
            GossipAction::Noop
            | GossipAction::AwaitingRemainder
            | GossipAction::AnnounceFinished => {
                trace!(item=%item_id, %sender, %action, "received gossip request");
                // Send a response to the sender indicating we already hold the item.
                let reply = Message::GossipResponse {
                    item_id: item_id.clone(),
                    is_already_held: true,
                };
                let mut effects = effect_builder.send_message(sender, reply).ignore();

                if action == GossipAction::AnnounceFinished {
                    effects.extend(
                        effect_builder
                            .announce_finished_gossiping(item_id.clone())
                            .ignore(),
                    );
                }

                effects
            }
        };
        if T::REQUIRES_GOSSIP_RECEIVED_ANNOUNCEMENT {
            effects.extend(
                effect_builder
                    .announce_gossip_received(item_id, sender)
                    .ignore(),
            );
        }
        effects
    }

    /// Handles an incoming gossip response from a peer on the network.
    fn handle_gossip_response(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        item_id: T::Id,
        is_already_held: bool,
        sender: NodeId,
    ) -> Effects<Event<T>> {
        let mut effects: Effects<_> = Effects::new();
        let action = if is_already_held {
            self.table.already_infected(&item_id, sender)
        } else {
            if !T::ID_IS_COMPLETE_ITEM {
                // `sender` doesn't hold the full item; get the item from the component responsible
                // for holding it, then send it to `sender`.
                effects.extend((self.get_from_holder)(
                    effect_builder,
                    item_id.clone(),
                    sender,
                ));
            }
            self.table.we_infected(&item_id, sender)
        };

        match action {
            GossipAction::ShouldGossip(should_gossip) => effects.extend(self.gossip(
                effect_builder,
                item_id,
                should_gossip.target,
                should_gossip.count,
                should_gossip.exclude_peers,
            )),
            GossipAction::Noop => (),
            GossipAction::AnnounceFinished => {
                effects.extend(effect_builder.announce_finished_gossiping(item_id).ignore())
            }
            GossipAction::GetRemainder { .. } => {
                error!("shouldn't try to get remainder as result of receiving a gossip response");
            }
            GossipAction::AwaitingRemainder => {
                warn!(
                    "shouldn't have gossiped if we don't hold the complete item - possible \
                    significant latency, or malicious peer"
                );
            }
        }

        effects
    }

    /// Handles the `Ok` case for a `Result` of attempting to get the item from the component
    /// responsible for holding it, in order to send it to the requester.
    fn got_from_holder(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        item: T,
        requester: NodeId,
    ) -> Effects<Event<T>> {
        let message = Message::Item(Arc::new(item));
        effect_builder.send_message(requester, message).ignore()
    }

    /// Handles the `Err` case for a `Result` of attempting to get the item from the component
    /// responsible for holding it.
    fn failed_to_get_from_holder(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        item_id: T::Id,
        error: String,
    ) -> Effects<Event<T>> {
        error!(
            "finished gossiping {} since failed to get from store: {}",
            item_id, error
        );

        if self.table.force_finish(&item_id) {
            // Currently the only consumer of the `FinishedGossiping` announcement is the
            // `DeployBuffer`, and it's not a problem to it if the deploy is unavailable in storage
            // as it should fail to retrieve the deploy too and hence not propose it.  If we need to
            // differentiate between successful termination of gossiping and this forced termination
            // in the future, we can emit a new announcement variant here.
            return effect_builder.announce_finished_gossiping(item_id).ignore();
        }

        Effects::new()
    }

    fn handle_get_item_request(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        item_id: T::Id,
        requester: NodeId,
    ) -> Effects<Event<T>> {
        (self.get_from_holder)(effect_builder, item_id, requester)
    }

    fn handle_item_received_from_peer(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        item: Arc<T>,
        sender: NodeId,
    ) -> Effects<Event<T>> {
        effect_builder
            .announce_item_body_received_via_gossip(item, sender)
            .ignore()
    }

    /// Updates the gossiper metrics from the state of the gossip table.
    fn update_gossip_table_metrics(&self) {
        self.metrics
            .table_items_current
            .set(self.table.items_current() as i64);
        self.metrics
            .table_items_finished
            .set(self.table.items_finished() as i64);
    }
}

impl<T, REv> Component<REv> for Gossiper<T, REv>
where
    T: GossiperItem + 'static,
    REv: ReactorEventT<T>,
{
    type Event = Event<T>;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        let effects = match event {
            Event::BeginGossipRequest(BeginGossipRequest {
                item_id,
                source,
                responder,
            }) => {
                let mut effects = self.handle_item_received(effect_builder, item_id, source);
                effects.extend(responder.respond(()).ignore());
                effects
            }
            Event::ItemReceived { item_id, source } => {
                self.handle_item_received(effect_builder, item_id, source)
            }
            Event::GossipedTo {
                item_id,
                requested_count,
                peers,
            } => self.gossiped_to(effect_builder, item_id, requested_count, peers),
            Event::CheckGossipTimeout { item_id, peer } => {
                self.check_gossip_timeout(effect_builder, item_id, peer)
            }
            Event::CheckGetFromPeerTimeout { item_id, peer } => {
                self.check_get_from_peer_timeout(effect_builder, item_id, peer)
            }
            Event::Incoming(GossiperIncoming::<T> { sender, message }) => match message {
                Message::Gossip(item_id) => self.handle_gossip(effect_builder, item_id, sender),
                Message::GossipResponse {
                    item_id,
                    is_already_held,
                } => self.handle_gossip_response(effect_builder, item_id, is_already_held, sender),
                Message::GetItem(item_id) => {
                    self.handle_get_item_request(effect_builder, item_id, sender)
                }
                Message::Item(item) => {
                    self.handle_item_received_from_peer(effect_builder, item, sender)
                }
            },
            Event::GetFromHolderResult {
                item_id,
                requester,
                result,
            } => match *result {
                Ok(item) => self.got_from_holder(effect_builder, item, requester),
                Err(error) => self.failed_to_get_from_holder(effect_builder, item_id, error),
            },
        };
        self.update_gossip_table_metrics();
        effects
    }

    fn name(&self) -> &str {
        COMPONENT_NAME
    }
}

impl<T: GossiperItem + 'static, REv: ReactorEventT<T>> Debug for Gossiper<T, REv> {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        formatter
            .debug_struct("Gossiper")
            .field("table", &self.table)
            .field("gossip_timeout", &self.gossip_timeout)
            .field("get_from_peer_timeout", &self.get_from_peer_timeout)
            .finish()
    }
}
