//! Very fast networking component used for testing and simulations.
//!
//! The `InMemoryNetwork` represents a full virtual network with flawless connectivity and delivery
//! by default.
//!
//! # Setup
//!
//! The network itself is managed by a `NetworkController` that can be used to create networking
//! components for nodes. Let's demonstrate this with an example in which we
//!
//! 1. Define a fictional "shouter" component to utilize the network.
//! 2. Create an application (in the form of a reactor) that connects this shouter to an in-memory
//!    network of nodes.
//! 3. Run a test that verifies everything is working.
//!
//! ```rust
//! # #![allow(dead_code)] // FIXME: Remove me
//! # use std::{collections::HashMap, fmt::{self, Formatter, Debug, Display}, ops::AddAssign,
//! #           time::Duration};
//! #
//! # use derive_more::From;
//! # use maplit::hashmap;
//! # use rand::Rng;
//! # use tracing::Span;
//! #
//! # use casperlabs_node::{components::{Component,
//! #                       in_memory_network::{InMemoryNetwork, NetworkController, NodeId}},
//! #                       effect::{Effect, EffectBuilder, EffectExt, Multiple,
//! #                       announcements::NetworkAnnouncement, requests::NetworkRequest},
//! #                       reactor::{self, EventQueueHandle, wrap_effects},
//! #                       testing::network::{Network, NetworkedReactor}};
//! #
//! # let mut runtime = tokio::runtime::Runtime::new().unwrap();
//! #
//! // Our network messages are just integers in this example.
//! type Message = u64;
//!
//! // When gossiping, always select exactly two nodes.
//! const TEST_GOSSIP_COUNT: usize = 2;
//!
//! // We will test with three nodes.
//! const TEST_NODE_COUNT: usize = 3;
//! # assert!(TEST_GOSSIP_COUNT < TEST_NODE_COUNT);
//!
//! /// The shouter component. Sends messages across the network and tracks incoming.
//! #[derive(Debug)]
//! struct Shouter {
//!     /// Values we will gossip.
//!     whispers: Vec<Message>,
//!     /// Values we will broadcast.
//!     shouts: Vec<Message>,
//!     /// Values we received.
//!     received: Vec<(NodeId, Message)>,
//! }
//!
//! impl Shouter {
//!     /// Returns the totals of each message value received. Used for verification in testing.
//!     fn count_messages(&self) -> HashMap<Message, usize> {
//!         let mut totals = HashMap::<Message, usize>::new();
//!
//!         for (_node_id, message) in &self.received {
//!             totals.entry(*message).or_default().add_assign(1);
//!         }
//!
//!         totals
//!     }
//! }
//!
//! #[derive(Debug, From)]
//! enum ShouterEvent<NodeId, Message> {
//!     #[from]
//!     // We received a new message via the network.
//!     Net(NetworkAnnouncement<NodeId, Message>),
//!     // Ready to send another message.
//!     #[from]
//!     ReadyToSend,
//! }
//!
//! impl Shouter {
//!     /// Creates a new shouter.
//!     fn new<REv: Send, I: 'static, P: 'static>(effect_builder: EffectBuilder<REv>)
//!             -> (Self, Multiple<Effect<ShouterEvent<I, P>>>) {
//!         (Shouter {
//!             whispers: Vec::new(),
//!             shouts: Vec::new(),
//!             received: Vec::new(),
//!         }, effect_builder.immediately().event(|_| ShouterEvent::ReadyToSend))
//!     }
//! }
//!
//! // Besides its own events, the shouter is capable of receiving network messages.
//! impl<REv> Component<REv> for Shouter
//!     where REv: From<NetworkRequest<NodeId, Message>> + Send
//! {
//!     type Event = ShouterEvent<NodeId, Message>;
//!
//!     fn handle_event<R: Rng + ?Sized>(&mut self,
//!                                      effect_builder: EffectBuilder<REv>,
//!                                      _rng: &mut R,
//!                                      event: Self::Event
//!     ) -> Multiple<Effect<Self::Event>> {
//!         match event {
//!             ShouterEvent::Net(NetworkAnnouncement::MessageReceived { sender, payload }) => {
//!                 // Record the message we received.
//!                 self.received.push((sender, payload));
//!                 Default::default()
//!             }
//!             ShouterEvent::ReadyToSend => {
//!                 // If we need to whisper something, do so.
//!                 if let Some(msg) = self.whispers.pop() {
//!                     return effect_builder.gossip_message(msg,
//!                                                          TEST_GOSSIP_COUNT,
//!                                                          Default::default())
//!                         .event(|_| ShouterEvent::ReadyToSend);
//!                 }
//!                 // Shouts get broadcast.
//!                 if let Some(msg) = self.shouts.pop() {
//!                     return effect_builder.broadcast_message(msg)
//!                         .event(|_| ShouterEvent::ReadyToSend);
//!                 }
//!                 Default::default()
//!             }
//!         }
//!     }
//! }
//!
//! /// The reactor ties the shouter component to a network.
//! #[derive(Debug)]
//! struct Reactor {
//!     /// The connection to the internal network.
//!     net: InMemoryNetwork<u64>,
//!     /// Local shouter instance.
//!     shouter: Shouter,
//! }
//!
//! /// Reactor event
//! #[derive(Debug, From)]
//! enum Event {
//!    /// Asked to perform a network action.
//!    #[from]
//!    Request(NetworkRequest<NodeId, Message>),
//!    /// Event for the shouter.
//!    #[from]
//!    Shouter(ShouterEvent<NodeId, Message>),
//!    /// Notified of some network event.
//!    #[from]
//!    Announcement(NetworkAnnouncement<NodeId, Message>)
//! };
//! #
//! # impl Display for Event {
//! #   fn fmt(&self, fmt: &mut Formatter<'_>) -> fmt::Result {
//! #       Debug::fmt(self, fmt)
//! #   }
//! # }
//! #
//! # impl<I, P> Display for ShouterEvent<I, P>
//! #     where I: Debug,
//! #           P: Debug
//! # {
//! #   fn fmt(&self, fmt: &mut Formatter<'_>) -> fmt::Result {
//! #       Debug::fmt(self, fmt)
//! #   }
//! # }
//!
//! impl reactor::Reactor for Reactor {
//!     type Event = Event;
//!     type Config = ();
//!     type Error = ();
//!
//!     fn new(_cfg: Self::Config,
//!            event_queue: EventQueueHandle<Self::Event>,
//!            _span: &Span
//!     ) -> Result<(Self, Multiple<Effect<Self::Event>>), ()> {
//!         let effect_builder = EffectBuilder::new(event_queue);
//!         let (shouter, shouter_effect) = Shouter::new(effect_builder);
//!
//!         Ok((Reactor {
//!             // TODO: Consider whether we want to move RNG access all the way up, if possible.
//!             net: NetworkController::create_node(event_queue, &mut rand::thread_rng()),
//!             shouter,
//!         }, wrap_effects(From::from, shouter_effect)))
//!     }
//!
//!     fn dispatch_event(&mut self,
//!                       effect_builder: EffectBuilder<Event>,
//!                       event: Event
//!     ) -> Multiple<Effect<Event>> {
//!          let mut rng = rand::thread_rng(); // FIXME: RNGs should be passed in.
//!          match event {
//!              Event::Announcement(anc) => { wrap_effects(From::from,
//!                  self.shouter.handle_event(effect_builder, &mut rng, anc.into())
//!              )}
//!              Event::Request(req) => { wrap_effects(From::from,
//!                  self.net.handle_event(effect_builder, &mut rng, req.into())
//!              )}
//!              Event::Shouter(ev) => { wrap_effects(From::from,
//!                  self.shouter.handle_event(effect_builder, &mut rng, ev)
//!              )}
//!          }
//!     }
//! }
//!
//! impl NetworkedReactor for Reactor {
//!   type NodeId = NodeId;
//!
//!   fn node_id(&self) -> NodeId {
//!       self.net.node_id()
//!   }
//! }
//!
//! // We can finally run the tests:
//!
//! # // We need to be inside a tokio runtime to execute `async` code.
//! # runtime.block_on(async move {
//! #
//! // Create a new network controller that manages the network itself. This will register the
//! // network controller on the current thread and allow initialization functions to find it.
//! NetworkController::<Message>::create_active();
//!
//! // We can now create the network of nodes, using the `testing::Network` and insert three nodes.
//! // Each node is given some data to send.
//! let mut net = Network::<Reactor>::new();
//! let (id1, n1) = net.add_node().await.unwrap();
//! n1.reactor_mut().shouter.shouts.push(1);
//! n1.reactor_mut().shouter.shouts.push(2);
//! n1.reactor_mut().shouter.whispers.push(3);
//! n1.reactor_mut().shouter.whispers.push(4);
//!
//! let (id2, n2) = net.add_node().await.unwrap();
//! n2.reactor_mut().shouter.shouts.push(6);
//! n2.reactor_mut().shouter.whispers.push(4);
//!
//! let (id3, n3) = net.add_node().await.unwrap();
//! n3.reactor_mut().shouter.whispers.push(8);
//! n3.reactor_mut().shouter.shouts.push(1);
//!
//! net.settle(Duration::from_secs(1)).await;
//! assert_eq!(net.nodes().len(), TEST_NODE_COUNT);
//!
//! let mut global_count = HashMap::<Message, usize>::new();
//! for node_id in &[id1, id2, id3] {
//!     let totals = net.nodes()[node_id].reactor().shouter.count_messages();
//!
//!     // The broadcast values should be the same for each node:
//!     assert_eq!(totals[&1], 2);
//!     assert_eq!(totals[&2], 1);
//!     assert_eq!(totals[&6], 1);
//!
//!     // Add values to global_count count.
//!     for (val, count) in totals.into_iter() {
//!         global_count.entry(val).or_default().add_assign(count);
//!     }
//! }
//!
//! assert_eq!(global_count, hashmap!{
//!     1 => 2 * TEST_NODE_COUNT,
//!     2 => TEST_NODE_COUNT,
//!     3 => TEST_GOSSIP_COUNT,
//!     4 => 2 * TEST_GOSSIP_COUNT,
//!     6 => TEST_NODE_COUNT,
//!     8 => TEST_GOSSIP_COUNT,
//! });
//!
//! // It's good form to remove the active network.
//! NetworkController::<Message>::remove_active();
//!
//! # }); // end of tokio::block_on
//! ```

use std::{
    any::Any,
    collections::{HashMap, HashSet},
    fmt::Display,
    sync::{Arc, Mutex, RwLock},
};

use lazy_static::lazy_static;
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

/// The node ID type used by the in-memory network.
pub type NodeId = u64;

lazy_static! {
    /// The currently active network as a thread local.
    ///
    /// The type is dynamic, every network can be of a distinct type when the payload `P` differs.
    static ref ACTIVE_NETWORK: Mutex<Option<Box<dyn Any + Send + Sync>>> = Mutex::new(None);
}

/// The network controller is used to control the network topology (e.g. adding and removing nodes).
#[derive(Debug, Default)]
pub struct NetworkController<P> {
    /// Channels for network communication.
    nodes: Network<P>,
}

impl<P> NetworkController<P>
where
    P: 'static + Send,
{
    /// Create a new, empty network.
    fn new() -> Self {
        NetworkController {
            nodes: Default::default(),
        }
    }

    /// Creates a new, empty network controller and sets it as active.
    ///
    /// # Panics
    ///
    /// Panics if the internal lock has been poisoned.
    pub fn create_active() {
        ACTIVE_NETWORK
            .lock()
            .expect("active network lock has been poisoned")
            .replace(Box::new(Self::new()));
    }

    /// Removes the active network.
    ///
    /// # Panics
    ///
    /// Panics if the internal lock has been poisoned, a network with the wrong type of message was
    /// removed or if there was no network at at all.
    pub fn remove_active() {
        assert!(
            ACTIVE_NETWORK
                .lock()
                .expect("active network lock has been poisoned")
                .take()
                .expect("tried to remove non-existant network")
                .is::<Self>(),
            "removed network was of wrong type"
        );
    }

    /// Creates an in-memory network component on the active network.
    ///
    /// # Panics
    ///
    /// Panics if the internal lock has been poisoned, there is no active network or the active
    /// network is not of the correct message type.
    pub fn create_node<REv, R>(
        event_queue: EventQueueHandle<REv>,
        rng: &mut R,
    ) -> InMemoryNetwork<P>
    where
        R: Rng,
        REv: From<NetworkAnnouncement<NodeId, P>> + Send,
    {
        ACTIVE_NETWORK
            .lock()
            .expect("active network lock has been poisoned")
            .as_mut()
            .expect("tried to create node without active network set")
            .downcast_mut::<Self>()
            .expect("active network has wrong message type")
            .create_node_local(event_queue, rng)
    }

    /// Creates a new networking node with a random node ID.
    ///
    /// Returns the already connected new networking component for new node.
    pub(crate) fn create_node_local<REv, R>(
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

/// Networking component connected to an in-memory network.
#[derive(Debug)]
pub struct InMemoryNetwork<P> {
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

    /// Returns this node's ID.
    #[inline]
    pub fn node_id(&self) -> NodeId {
        self.node_id
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
                    let chosen: HashSet<_> = guard
                        .keys()
                        .filter(|k| !exclude.contains(k))
                        .cloned()
                        .choose_multiple(rng, count)
                        .into_iter()
                        .collect();
                    // Not terribly efficient, but will always get us the maximum amount of nodes.
                    for &dest in chosen.iter() {
                        self.send(&guard, dest, payload.clone());
                    }
                    responder.respond(chosen).ignore()
                } else {
                    error!("network lock has been poisoned");
                    responder.respond(Default::default()).ignore()
                }
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
            .schedule(announce, QueueKind::NetworkIncoming)
            .await;
    }

    debug!("receiver shutting down")
}
