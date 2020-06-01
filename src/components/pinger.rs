//! Networking demonstration component.
//!
//! The pinger component sends a broadcast to all other nodes every five seconds with a `Ping`. When
//! receiving a `Ping`, it will respond with a `Pong` to the sender.

use std::collections::HashSet;
use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::{
    components::{small_network::NodeId, Component},
    effect::{requests::NetworkRequest, Effect, EffectBuilder, EffectExt, Multiple},
    utils::DisplayIter,
};

/// The pinger components.
///
/// Keeps track internally of nodes it knows are up.
#[derive(Debug)]
pub(crate) struct Pinger {
    /// Nodes that respondes to the most recent ping sent.
    responsive_nodes: HashSet<NodeId>,
    /// Increasing ping counter.
    ping_counter: u32,
}

/// Interval in which to send pings.
const PING_INTERVAL: Duration = Duration::from_secs(3);

/// Network message used by the pinger.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) enum Message {
    /// Ping with counter.
    Ping(u32),
    /// Pong with counter.
    Pong(u32),
}

/// Pinger component event.
#[derive(Debug)]
pub(crate) enum Event {
    /// An incoming network message.
    MessageReceived { sender: NodeId, msg: Message },
    /// The next round of pings should be sent out.
    Timer,
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Message::Ping(ctr) => write!(f, "ping({})", ctr),
            Message::Pong(ctr) => write!(f, "pong({})", ctr),
        }
    }
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Event::MessageReceived { sender, msg } => write!(f, "msg from {}: {}", sender, msg),
            Event::Timer => write!(f, "timer"),
        }
    }
}

impl<REv> Component<REv> for Pinger
where
    REv: From<Event> + Send + From<NetworkRequest<NodeId, Message>>,
{
    type Event = Event;

    fn handle_event(
        &mut self,
        eb: EffectBuilder<REv>,
        event: Self::Event,
    ) -> Multiple<Effect<Self::Event>> {
        match event {
            Event::Timer => self.send_pings(eb),
            Event::MessageReceived {
                sender,
                msg: Message::Ping(counter),
            } => {
                // When receiving a `Ping`, do nothing but reply with a `Pong`.
                eb.send_message(sender, Message::Pong(counter)).ignore()
            }
            Event::MessageReceived {
                sender,
                msg: Message::Pong(counter),
            } => {
                // We've received a pong, if it is valid (same counter value), process it.
                if counter == self.ping_counter {
                    self.responsive_nodes.insert(sender);
                } else {
                    info!("received stale ping({}) from {}", counter, sender);
                }

                Default::default()
            }
        }
    }
}

impl Pinger {
    /// Create and initialize a new pinger.
    pub(crate) fn new<REv: From<Event> + Send + From<NetworkRequest<NodeId, Message>>>(
        eb: EffectBuilder<REv>,
    ) -> (Self, Multiple<Effect<Event>>) {
        let mut pinger = Pinger {
            responsive_nodes: HashSet::new(),
            ping_counter: 0,
        };

        // We send out a round of pings immediately on construction.
        let init = pinger.send_pings(eb);

        (pinger, init)
    }

    /// Broadcast a ping and set a timer for the next broadcast.
    fn send_pings<REv: From<Event> + Send + From<NetworkRequest<NodeId, Message>>>(
        &mut self,
        eb: EffectBuilder<REv>,
    ) -> Multiple<Effect<Event>> {
        info!(
            "starting new ping round, previously saw {{{}}}",
            DisplayIter::new(self.responsive_nodes.iter())
        );

        // We increment the counter and clear pings beforehand, thus causing all pongs that are
        // still in flight to be timeouts.
        self.ping_counter += 1;
        self.responsive_nodes.clear();

        let mut effects: Multiple<Effect<Event>> = Default::default();
        effects.extend(
            eb.broadcast_message(Message::Ping(self.ping_counter))
                .ignore(),
        );
        effects.extend(eb.set_timeout(PING_INTERVAL).event(|_| Event::Timer));

        effects
    }
}
