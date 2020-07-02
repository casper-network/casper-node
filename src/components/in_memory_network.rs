//! In-memory networking component
//!
//! A very fast networking component used for testing and simulations.

// FIXME: Remove when in use.
#![allow(dead_code)]

use std::{
    collections::HashMap,
    fmt::Display,
    sync::{Arc, RwLock},
};

use rand::{seq::IteratorRandom, Rng};
use tokio::sync::mpsc::{self, error::SendError};
use tracing::{debug, error, info, warn};

use crate::{
    components::Component,
    effect::{
        announcements::NetworkAnnouncement, requests::NetworkRequest, Effect, EffectBuilder,
        EffectExt, Multiple,
    },
    reactor::{EventQueueHandle, QueueKind},
};

type Network<P> = Arc<RwLock<HashMap<NodeId, mpsc::UnboundedSender<(NodeId, P)>>>>;
type NodeId = u64;

/// The network controller is used to control the network topology (e.g. adding and removing nodes).
#[derive(Debug)]
pub(crate) struct NetworkController<P> {
    /// Channels for network communication.
    nodes: Network<P>,
}

impl<P> NetworkController<P>
where
    P: 'static + Send,
{
    /// Creates a new, empty network inside a network controller.
    pub(crate) fn new() -> Self {
        NetworkController {
            nodes: Default::default(),
        }
    }

    /// Creates a new networking node with a random node ID.
    ///
    /// Returns the already connected new networking component for new node.
    pub(crate) fn create_node<REv, R>(
        &self,
        event_queue: EventQueueHandle<REv>,
        rng: &mut R,
    ) -> InMemoryNetwork<P>
    where
        R: Rng,
        REv: From<NetworkAnnouncement<NodeId, P>> + Send,
    {
        InMemoryNetwork::new(event_queue, rng.gen(), self.nodes.clone())
    }
}

#[derive(Debug)]
pub(crate) struct InMemoryNetwork<P> {
    /// Our node id.
    node_id: NodeId,

    /// The nodes map, contains the incoming channel for each virtual node.
    nodes: Network<P>,
}

impl<P> InMemoryNetwork<P>
where
    P: 'static + Send,
{
    fn new<REv>(event_queue: EventQueueHandle<REv>, node_id: NodeId, nodes: Network<P>) -> Self
    where
        REv: From<NetworkAnnouncement<NodeId, P>> + Send,
    {
        let (sender, receiver) = mpsc::unbounded_channel();

        // Sanity check, ensure that we do not create duplicate nodes.
        {
            let mut nodes_write = nodes.write().expect("network lock poisoned");
            assert!(!nodes_write.contains_key(&node_id));
            nodes_write.insert(node_id, sender);
        }

        tokio::spawn(receiver_task(event_queue, receiver));

        InMemoryNetwork { node_id, nodes }
    }
}

impl<P> InMemoryNetwork<P>
where
    P: Display,
{
    /// Internal helper, sends a payload to a node, ignoring but logging all errors.
    fn send(
        &self,
        nodes: &HashMap<NodeId, mpsc::UnboundedSender<(NodeId, P)>>,
        dest: NodeId,
        payload: P,
    ) {
        match nodes.get(&dest) {
            Some(sender) => {
                if let Err(SendError((_, msg))) = sender.send((self.node_id, payload)) {
                    warn!(%dest, %msg, "could not send message (send error)");

                    // We do nothing else, the message is just dropped.
                }
            }
            None => info!(%dest, %payload, "dropping message to non-existant recipient"),
        }
    }
}

impl<P, REv> Component<REv> for InMemoryNetwork<P>
where
    P: Display + Clone,
{
    type Event = NetworkRequest<NodeId, P>;

    fn handle_event<R: Rng + ?Sized>(
        &mut self,
        _effect_builder: EffectBuilder<REv>,
        rng: &mut R,
        event: Self::Event,
    ) -> Multiple<Effect<Self::Event>> {
        match event {
            NetworkRequest::SendMessage {
                dest,
                payload,
                responder,
            } => {
                if let Ok(guard) = self.nodes.read() {
                    self.send(&guard, dest, payload);
                } else {
                    error!("network lock has been poisoned")
                };

                responder.respond(()).ignore()
            }
            NetworkRequest::Broadcast { payload, responder } => {
                if let Ok(guard) = self.nodes.read() {
                    for dest in guard.keys() {
                        self.send(&guard, *dest, payload.clone());
                    }
                } else {
                    error!("network lock has been poisoned")
                };

                responder.respond(()).ignore()
            }
            NetworkRequest::Gossip {
                payload,
                count,
                exclude,
                responder,
            } => {
                if let Ok(guard) = self.nodes.read() {
                    // Not terribly efficient, but will always get us the maximum amount of nodes.
                    for dest in guard
                        .keys()
                        .filter(|k| !exclude.contains(k))
                        .choose_multiple(rng, count)
                    {
                        self.send(&guard, *dest, payload.clone());
                    }
                } else {
                    error!("network lock has been poisoned")
                };

                responder.respond(()).ignore()
            }
        }
    }
}

async fn receiver_task<REv, P>(
    event_queue: EventQueueHandle<REv>,
    mut receiver: mpsc::UnboundedReceiver<(NodeId, P)>,
) where
    REv: From<NetworkAnnouncement<NodeId, P>>,
    P: 'static + Send,
{
    while let Some((sender, payload)) = receiver.recv().await {
        let announce = NetworkAnnouncement::MessageReceived { sender, payload };

        event_queue
            .schedule(announce.into(), QueueKind::NetworkIncoming)
            .await;
    }

    debug!("receiver shutting down")
}
