mod config;
#[cfg(test)]
mod error;
mod event;
mod gossip_item;
mod gossip_table;
mod item_provider;
mod message;
mod metrics;
mod provider_impls;
mod tests;

use std::{
    collections::HashSet,
    fmt::{self, Debug, Formatter},
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
    types::NodeId,
    utils::Source,
    NodeRng,
};
pub(crate) use config::Config;
pub(crate) use event::Event;
pub(crate) use gossip_item::{GossipItem, LargeGossipItem, SmallGossipItem};
use gossip_table::{GossipAction, GossipTable};
use item_provider::ItemProvider;
pub(crate) use message::Message;
use metrics::Metrics;

/// The component which gossips to peers and handles incoming gossip messages from peers.
#[allow(clippy::type_complexity)]
pub(crate) struct Gossiper<const ID_IS_COMPLETE_ITEM: bool, T>
where
    T: GossipItem + 'static,
{
    table: GossipTable<T::Id>,
    gossip_timeout: Duration,
    get_from_peer_timeout: Duration,
    name: &'static str,
    metrics: Metrics,
}

impl<const ID_IS_COMPLETE_ITEM: bool, T: GossipItem + 'static> Gossiper<ID_IS_COMPLETE_ITEM, T> {
    /// Constructs a new gossiper component.
    ///
    /// Must be supplied with a name, which should be a snake-case identifier to disambiguate the
    /// specific gossiper from other potentially present gossipers.
    pub(crate) fn new(
        name: &'static str,
        config: Config,
        registry: &Registry,
    ) -> Result<Self, prometheus::Error> {
        Ok(Gossiper {
            table: GossipTable::new(config),
            gossip_timeout: config.gossip_request_timeout().into(),
            get_from_peer_timeout: config.get_remainder_timeout().into(),
            name,
            metrics: Metrics::new(name, registry)?,
        })
    }

    /// Handles a new item received from a peer or client for which we should begin gossiping.
    ///
    /// Note that this doesn't include items gossiped to us; those are handled in `handle_gossip()`.
    fn handle_item_received<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        item_id: T::Id,
        source: Source,
        target: GossipTarget,
    ) -> Effects<Event<T>>
    where
        REv: From<NetworkRequest<Message<T>>> + From<GossiperAnnouncement<T>> + Send,
    {
        debug!(item=%item_id, %source, "received new gossip item");
        match self
            .table
            .new_complete_data(&item_id, source.node_id(), target)
        {
            GossipAction::ShouldGossip(should_gossip) => {
                self.metrics.items_received.inc();
                Self::gossip(
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
    fn gossip<REv>(
        effect_builder: EffectBuilder<REv>,
        item_id: T::Id,
        gossip_target: GossipTarget,
        count: usize,
        exclude_peers: HashSet<NodeId>,
    ) -> Effects<Event<T>>
    where
        REv: From<NetworkRequest<Message<T>>> + Send,
    {
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
    fn gossiped_to<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        item_id: T::Id,
        requested_count: usize,
        peers: HashSet<NodeId>,
    ) -> Effects<Event<T>>
    where
        REv: From<GossiperAnnouncement<T>> + Send,
    {
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
    fn check_gossip_timeout<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        item_id: T::Id,
        peer: NodeId,
    ) -> Effects<Event<T>>
    where
        REv: From<NetworkRequest<Message<T>>> + From<GossiperAnnouncement<T>> + Send,
    {
        match self.table.check_timeout(&item_id, peer) {
            GossipAction::ShouldGossip(should_gossip) => Self::gossip(
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
    fn check_get_from_peer_timeout<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        item_id: T::Id,
        peer: NodeId,
    ) -> Effects<Event<T>>
    where
        REv: From<NetworkRequest<Message<T>>> + From<GossiperAnnouncement<T>> + Send,
    {
        match self.table.remove_holder_if_unresponsive(&item_id, peer) {
            GossipAction::ShouldGossip(should_gossip) => Self::gossip(
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

    /// Handles an incoming gossip request from a peer on the network, after having registered the
    /// item in the gossip table.
    fn handle_gossip<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        item_id: T::Id,
        sender: NodeId,
        action: GossipAction,
    ) -> Effects<Event<T>>
    where
        REv: From<NetworkRequest<Message<T>>> + From<GossiperAnnouncement<T>> + Send,
    {
        let mut effects = match action {
            GossipAction::ShouldGossip(should_gossip) => {
                debug!(item=%item_id, %sender, %should_gossip, "received gossip request");
                self.metrics.items_received.inc();
                // Gossip the item ID.
                let mut effects = Self::gossip(
                    effect_builder,
                    item_id.clone(),
                    should_gossip.target,
                    should_gossip.count,
                    should_gossip.exclude_peers,
                );

                // If this is a new complete item to us, announce it.
                if ID_IS_COMPLETE_ITEM && !should_gossip.is_already_held {
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
    fn handle_gossip_response<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        item_id: T::Id,
        is_already_held: bool,
        sender: NodeId,
    ) -> Effects<Event<T>>
    where
        REv: From<NetworkRequest<Message<T>>>
            + From<StorageRequest>
            + From<GossiperAnnouncement<T>>
            + Send,
        Self: ItemProvider<T>,
    {
        let mut effects: Effects<_> = Effects::new();
        let action = if is_already_held {
            self.table.already_infected(&item_id, sender)
        } else {
            if !ID_IS_COMPLETE_ITEM {
                // `sender` doesn't hold the full item; get the item from the component responsible
                // for holding it, then send it to `sender`.
                effects.extend(Self::get_from_storage(
                    effect_builder,
                    item_id.clone(),
                    sender,
                ));
            }
            self.table.we_infected(&item_id, sender)
        };

        match action {
            GossipAction::ShouldGossip(should_gossip) => effects.extend(Self::gossip(
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

    /// Handles the `Ok` case for a `Result` of attempting to get the item from storage in order to
    /// send it to the requester.
    fn got_from_storage<REv>(
        effect_builder: EffectBuilder<REv>,
        item: T,
        requester: NodeId,
    ) -> Effects<Event<T>>
    where
        REv: From<NetworkRequest<Message<T>>> + Send,
    {
        let message = Message::Item(Box::new(item));
        effect_builder.send_message(requester, message).ignore()
    }

    /// Handles the `Err` case for a `Result` of attempting to get the item from storage.
    fn failed_to_get_from_holder<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        item_id: T::Id,
        error: String,
    ) -> Effects<Event<T>>
    where
        REv: From<GossiperAnnouncement<T>> + Send,
    {
        error!(
            "finished gossiping {} since failed to get from storage: {}",
            item_id, error
        );

        if self.table.force_finish(&item_id) {
            return effect_builder.announce_finished_gossiping(item_id).ignore();
        }

        Effects::new()
    }

    fn handle_get_item_request<REv>(
        effect_builder: EffectBuilder<REv>,
        item_id: T::Id,
        requester: NodeId,
    ) -> Effects<Event<T>>
    where
        REv: From<StorageRequest> + Send,
        Self: ItemProvider<T>,
    {
        Self::get_from_storage(effect_builder, item_id, requester)
    }

    fn handle_item_received_from_peer<REv>(
        effect_builder: EffectBuilder<REv>,
        item: Box<T>,
        sender: NodeId,
    ) -> Effects<Event<T>>
    where
        REv: From<GossiperAnnouncement<T>> + Send,
    {
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

impl<T, REv> Component<REv> for Gossiper<false, T>
where
    T: LargeGossipItem + 'static,
    REv: From<NetworkRequest<Message<T>>>
        + From<StorageRequest>
        + From<GossiperAnnouncement<T>>
        + Send,
    Self: ItemProvider<T>,
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
                target,
                responder,
            }) => {
                let mut effects =
                    self.handle_item_received(effect_builder, item_id, source, target);
                effects.extend(responder.respond(()).ignore());
                effects
            }
            Event::ItemReceived {
                item_id,
                source,
                target,
            } => self.handle_item_received(effect_builder, item_id, source, target),
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
                Message::Gossip(item_id) => {
                    let action = self.table.new_data_id(&item_id, sender);
                    self.handle_gossip(effect_builder, item_id, sender, action)
                }
                Message::GossipResponse {
                    item_id,
                    is_already_held,
                } => self.handle_gossip_response(effect_builder, item_id, is_already_held, sender),
                Message::GetItem(item_id) => {
                    Self::handle_get_item_request(effect_builder, item_id, sender)
                }
                Message::Item(item) => {
                    Self::handle_item_received_from_peer(effect_builder, item, sender)
                }
            },
            Event::GetFromStorageResult {
                item_id,
                requester,
                result,
            } => match *result {
                Ok(item) => Self::got_from_storage(effect_builder, item, requester),
                Err(error) => self.failed_to_get_from_holder(effect_builder, item_id, error),
            },
        };
        self.update_gossip_table_metrics();
        effects
    }

    fn name(&self) -> &str {
        self.name
    }
}

impl<T, REv> Component<REv> for Gossiper<true, T>
where
    T: SmallGossipItem + 'static,
    REv: From<NetworkRequest<Message<T>>>
        + From<StorageRequest>
        + From<GossiperAnnouncement<T>>
        + Send,
    Self: ItemProvider<T>,
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
                target,
                responder,
            }) => {
                let mut effects =
                    self.handle_item_received(effect_builder, item_id, source, target);
                effects.extend(responder.respond(()).ignore());
                effects
            }
            Event::ItemReceived {
                item_id,
                source,
                target,
            } => self.handle_item_received(effect_builder, item_id, source, target),
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
                Message::Gossip(item_id) => {
                    let target = <T as SmallGossipItem>::id_as_item(&item_id).gossip_target();
                    let action = self.table.new_complete_data(&item_id, Some(sender), target);
                    self.handle_gossip(effect_builder, item_id, sender, action)
                }
                Message::GossipResponse {
                    item_id,
                    is_already_held,
                } => self.handle_gossip_response(effect_builder, item_id, is_already_held, sender),
                Message::GetItem(item_id) => {
                    Self::handle_get_item_request(effect_builder, item_id, sender)
                }
                Message::Item(item) => {
                    Self::handle_item_received_from_peer(effect_builder, item, sender)
                }
            },
            Event::GetFromStorageResult {
                item_id,
                requester,
                result,
            } => match *result {
                Ok(item) => Self::got_from_storage(effect_builder, item, requester),
                Err(error) => self.failed_to_get_from_holder(effect_builder, item_id, error),
            },
        };
        self.update_gossip_table_metrics();
        effects
    }

    fn name(&self) -> &str {
        self.name
    }
}

impl<const ID_IS_COMPLETE_ITEM: bool, T: GossipItem + 'static> Debug
    for Gossiper<ID_IS_COMPLETE_ITEM, T>
{
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        formatter
            .debug_struct(self.name)
            .field("table", &self.table)
            .field("gossip_timeout", &self.gossip_timeout)
            .field("get_from_peer_timeout", &self.get_from_peer_timeout)
            .finish()
    }
}

impl<const ID_IS_COMPLETE_ITEM: bool, T: GossipItem + 'static> DataSize
    for Gossiper<ID_IS_COMPLETE_ITEM, T>
{
    const IS_DYNAMIC: bool = true;

    const STATIC_HEAP_SIZE: usize = 0;

    #[inline]
    fn estimate_heap_size(&self) -> usize {
        let Gossiper {
            table,
            gossip_timeout,
            get_from_peer_timeout,
            name,
            metrics: _,
        } = self;

        table.estimate_heap_size()
            + gossip_timeout.estimate_heap_size()
            + get_from_peer_timeout.estimate_heap_size()
            + name.estimate_heap_size()
    }
}
