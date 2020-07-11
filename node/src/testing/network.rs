//! A network of test reactors.

use std::{
    collections::{hash_map::Entry, HashMap},
    fmt::{Debug, Display},
    hash::Hash,
    time::Duration,
};

use futures::future::{join_all, BoxFuture, FutureExt};
use tracing::debug;

use crate::reactor::{Finalize, Reactor, Runner};

/// A reactor with networking functionality.
pub trait NetworkedReactor: Sized {
    /// The node ID on the networking level.
    type NodeId: Eq + Hash + Clone + Display + Debug;

    /// Returns the node ID assigned to this specific reactor instance.
    fn node_id(&self) -> Self::NodeId;
}

/// Time interval for which to poll an observed testing network when no events have occurred.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// A network of multiple test reactors.
///
/// Nodes themselves are not run in the background, rather manual cranking is required through
/// `crank_all`. As an alternative, the `settle` and `settle_all` functions can be used to continue
/// cranking until a condition has been reached.
#[derive(Debug, Default)]
pub struct Network<R: Reactor + NetworkedReactor> {
    /// Current network.
    nodes: HashMap<<R as NetworkedReactor>::NodeId, Runner<R>>,
}

impl<R> Network<R>
where
    R: Reactor + NetworkedReactor,
    R::Config: Default,
    R::Error: From<prometheus::Error>,
{
    /// Creates a new networking node on the network using the default root node port.
    ///
    /// # Panics
    ///
    /// Panics if a duplicate node ID is being inserted. This should only happen in case a randomly
    /// generated ID collides.
    pub async fn add_node(&mut self) -> Result<(R::NodeId, &mut Runner<R>), R::Error> {
        self.add_node_with_config(Default::default()).await
    }
}

impl<R> Network<R>
where
    R: Reactor + NetworkedReactor,
    R::Error: From<prometheus::Error> + From<R::Error>,
{
    /// Creates a new network.
    pub fn new() -> Self {
        Network {
            nodes: HashMap::new(),
        }
    }

    /// Creates a new networking node on the network.
    ///
    /// # Panics
    ///
    /// Panics if a duplicate node ID is being inserted.
    pub async fn add_node_with_config(
        &mut self,
        cfg: R::Config,
    ) -> Result<(R::NodeId, &mut Runner<R>), R::Error> {
        let runner: Runner<R> = Runner::new(cfg).await?;

        let node_id = runner.reactor().node_id();

        let node_ref = match self.nodes.entry(node_id.clone()) {
            Entry::Occupied(_) => {
                // This happens in the event of the extremely unlikely hash collision, or if the
                // node ID was set manually.
                panic!("trying to insert a duplicate node {}", node_id)
            }
            Entry::Vacant(entry) => entry.insert(runner),
        };

        Ok((node_id, node_ref))
    }

    /// Crank all runners once, returning the number of events processed.
    pub async fn crank_all(&mut self) -> usize {
        join_all(self.nodes.values_mut().map(Runner::try_crank))
            .await
            .into_iter()
            .filter(Option::is_some)
            .count()
    }

    /// Process events on all nodes until all event queues are empty.
    ///
    /// Exits if `at_least` time has passed twice between events that have been processed.
    pub async fn settle(&mut self, at_least: Duration) {
        let mut no_events = false;
        loop {
            if self.crank_all().await == 0 {
                // Stop once we have no pending events and haven't had any for `at_least` duration.
                if no_events {
                    debug!(?at_least, "network has settled after");
                    break;
                } else {
                    no_events = true;
                    tokio::time::delay_for(at_least).await;
                }
            } else {
                no_events = false;
            }
        }
    }

    /// Runs the main loop of every reactor until a condition is true.
    pub async fn settle_on<F>(&mut self, f: F)
    where
        F: Fn(&HashMap<R::NodeId, Runner<R>>) -> bool,
    {
        loop {
            // Check condition.
            if f(&self.nodes) {
                debug!("network settled");
                break;
            }

            if self.crank_all().await == 0 {
                // No events processed, wait for a bit to avoid 100% cpu usage.
                tokio::time::delay_for(POLL_INTERVAL).await;
            }
        }
    }

    /// Returns the internal map of nodes.
    pub fn nodes(&self) -> &HashMap<R::NodeId, Runner<R>> {
        &self.nodes
    }
}

impl<R> Finalize for Network<R>
where
    R: Finalize + NetworkedReactor + Reactor + Send + 'static,
    R::NodeId: Send,
    R::Error: From<prometheus::Error>,
{
    fn finalize(self) -> BoxFuture<'static, ()> {
        // We support finalizing networks where the reactor itself can be finalized.

        async move {
            // Shutdown the sender of every reactor node to ensure the port is open again.
            for (_, node) in self.nodes.into_iter() {
                node.into_inner().finalize().await;
            }

            debug!("network finalized");
        }
        .boxed()
    }
}
