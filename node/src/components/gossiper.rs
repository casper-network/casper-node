mod config;
#[cfg(test)]
mod error;
mod event;
mod gossip_table;
mod message;
mod metrics;
mod tests;

use datasize::DataSize;
use prometheus::Registry;
use std::{
    collections::HashSet,
    convert::Infallible,
    fmt::{self, Debug, Formatter},
    time::Duration,
};
use tracing::{debug, error, warn};

use crate::{
    components::{fetcher::FetchedOrNotFound, Component},
    effect::{
        announcements::GossiperAnnouncement,
        incoming::GossiperIncoming,
        requests::{BeginGossipRequest, NetworkRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects,
    },
    protocol::Message as NodeMessage,
    types::{Deploy, DeployHash, DeployWithFinalizedApprovals, Item, NodeId},
    utils::Source,
    NodeRng,
};
pub(crate) use config::Config;
pub(crate) use event::Event;
use gossip_table::{GossipAction, GossipTable};
pub(crate) use message::Message;
use metrics::Metrics;

/// A helper trait whose bounds represent the requirements for a reactor event that `Gossiper` can
/// work with.
pub(crate) trait ReactorEventT<T>:
    From<Event<T>>
    + From<NetworkRequest<Message<T>>>
    + From<NetworkRequest<NodeMessage>>
    + From<StorageRequest>
    + From<GossiperAnnouncement<T>>
    + Send
    + 'static
where
    T: Item + 'static,
    <T as Item>::Id: 'static,
{
}

impl<REv, T> ReactorEventT<T> for REv
where
    T: Item + 'static,
    <T as Item>::Id: 'static,
    REv: From<Event<T>>
        + From<NetworkRequest<Message<T>>>
        + From<NetworkRequest<NodeMessage>>
        + From<StorageRequest>
        + From<GossiperAnnouncement<T>>
        + Send
        + 'static,
{
}

/// This function can be passed in to `Gossiper::new()` as the `get_from_holder` arg when
/// constructing a `Gossiper<Deploy>`.
pub(crate) fn get_deploy_from_storage<T: Item + 'static, REv: ReactorEventT<T>>(
    effect_builder: EffectBuilder<REv>,
    deploy_hash: DeployHash,
    sender: NodeId,
) -> Effects<Event<Deploy>> {
    effect_builder
        .get_deploys_from_storage(vec![deploy_hash])
        .event(move |mut results| {
            let result = if results.len() == 1 {
                results
                    .pop()
                    .unwrap()
                    .ok_or_else(|| String::from("failed to get deploy from storage"))
                    .map(DeployWithFinalizedApprovals::into_naive)
            } else {
                Err(String::from("expected a single result"))
            };
            Event::GetFromHolderResult {
                item_id: deploy_hash,
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
    T: Item + 'static,
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

impl<T: Item + 'static, REv: ReactorEventT<T>> Gossiper<T, REv> {
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
        count: usize,
        exclude_peers: HashSet<NodeId>,
    ) -> Effects<Event<T>> {
        let message = Message::Gossip(item_id);
        effect_builder
            .gossip_message(message, count, exclude_peers)
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
            effects.extend(effect_builder.announce_finished_gossiping(item_id).ignore());
        }

        // Set timeouts to check later that the specified peers all responded.
        for peer in peers {
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
                should_gossip.count,
                should_gossip.exclude_peers,
            ),

            GossipAction::GetRemainder { holder } => {
                // The previous peer failed to provide the item, so we still need to get it.  Send
                // a `GetRequest` to a different holder and set a timeout to check we got the
                // response.
                let request = match NodeMessage::new_get_request::<T>(&item_id) {
                    Ok(request) => request,
                    Err(error) => {
                        error!("failed to create get-request: {}", error);
                        // Treat this as if the holder didn't respond - i.e. try to get from a
                        // different holder.
                        return self.check_get_from_peer_timeout(effect_builder, item_id, holder);
                    }
                };
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

        debug!(item=%item_id, %sender, %action, "received gossip request");

        match action {
            GossipAction::ShouldGossip(should_gossip) => {
                self.metrics.items_received.inc();
                // Gossip the item ID.
                let mut effects = self.gossip(
                    effect_builder,
                    item_id,
                    should_gossip.count,
                    should_gossip.exclude_peers,
                );

                // If this is a new complete item to us, announce it.
                if T::ID_IS_COMPLETE_ITEM && !should_gossip.is_already_held {
                    debug!(item=%item_id, "announcing new complete gossip item received");
                    effects.extend(
                        effect_builder
                            .announce_complete_item_received_via_gossip(item_id)
                            .ignore(),
                    );
                }

                // Send a response to the sender indicating whether we already hold the item.
                let reply = Message::GossipResponse {
                    item_id,
                    is_already_held: should_gossip.is_already_held,
                };
                effects.extend(effect_builder.send_message(sender, reply).ignore());
                effects
            }
            GossipAction::GetRemainder { .. } => {
                self.metrics.items_received.inc();
                // Send a response to the sender indicating we want the full item from them, and set
                // a timeout for this response.
                let reply = Message::GossipResponse {
                    item_id,
                    is_already_held: false,
                };
                let mut effects = effect_builder.send_message(sender, reply).ignore();
                effects.extend(
                    effect_builder
                        .set_timeout(self.get_from_peer_timeout)
                        .event(move |_| Event::CheckGetFromPeerTimeout {
                            item_id,
                            peer: sender,
                        }),
                );
                effects
            }
            GossipAction::Noop
            | GossipAction::AwaitingRemainder
            | GossipAction::AnnounceFinished => {
                // Send a response to the sender indicating we already hold the item.
                let reply = Message::GossipResponse {
                    item_id,
                    is_already_held: true,
                };
                let mut effects = effect_builder.send_message(sender, reply).ignore();

                if action == GossipAction::AnnounceFinished {
                    effects.extend(effect_builder.announce_finished_gossiping(item_id).ignore());
                }

                effects
            }
        }
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
                effects.extend((self.get_from_holder)(effect_builder, item_id, sender));
            }
            self.table.we_infected(&item_id, sender)
        };

        match action {
            GossipAction::ShouldGossip(should_gossip) => effects.extend(self.gossip(
                effect_builder,
                item_id,
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
        match NodeMessage::new_get_response(&FetchedOrNotFound::Fetched(item)) {
            Ok(message) => effect_builder.send_message(requester, message).ignore(),
            Err(error) => {
                error!("failed to create get-response: {}", error);
                Effects::new()
            }
        }
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
            // `BlockProposer`, and it's not a problem to it if the deploy is unavailable in storage
            // as it should fail to retrieve the deploy too and hence not propose it.  If we need to
            // differentiate between successful termination of gossiping and this forced termination
            // in the future, we can emit a new announcement variant here.
            return effect_builder.announce_finished_gossiping(item_id).ignore();
        }

        Effects::new()
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
    T: Item + 'static,
    REv: ReactorEventT<T>,
{
    type Event = Event<T>;
    type ConstructionError = Infallible;

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
}

impl<T: Item + 'static, REv: ReactorEventT<T>> Debug for Gossiper<T, REv> {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        formatter
            .debug_struct("Gossiper")
            .field("table", &self.table)
            .field("gossip_timeout", &self.gossip_timeout)
            .field("get_from_peer_timeout", &self.get_from_peer_timeout)
            .finish()
    }
}
