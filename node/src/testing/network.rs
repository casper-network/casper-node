//! A network of test reactors.

use std::{
    collections::{hash_map::Entry, HashMap},
    fmt::{Debug, Display},
    hash::Hash,
    time::Duration,
};

use futures::future::{BoxFuture, FutureExt};
use rand::Rng;
use tokio::time;
use tracing::debug;
use tracing_futures::Instrument;

use super::ConditionCheckReactor;
use crate::{
    effect::{EffectBuilder, Effects},
    reactor::{Finalize, Reactor, Runner},
};

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
    nodes: HashMap<<R as NetworkedReactor>::NodeId, Runner<ConditionCheckReactor<R>>>,
}

impl<R> Network<R>
where
    R: Reactor + NetworkedReactor,
    R::Config: Default,
    <R as Reactor>::Error: Debug,
    R::Error: From<prometheus::Error>,
{
    /// Creates a new networking node on the network using the default root node port.
    ///
    /// # Panics
    ///
    /// Panics if a duplicate node ID is being inserted. This should only happen in case a randomly
    /// generated ID collides.
    pub async fn add_node<RNG: Rng + ?Sized>(
        &mut self,
        rng: &mut RNG,
    ) -> Result<(R::NodeId, &mut Runner<ConditionCheckReactor<R>>), R::Error> {
        self.add_node_with_config(Default::default(), rng).await
    }

    /// Adds `count` new nodes to the network, and returns their IDs.
    pub async fn add_nodes<RNG: Rng + ?Sized>(
        &mut self,
        rng: &mut RNG,
        count: usize,
    ) -> Vec<R::NodeId> {
        let mut node_ids = vec![];
        for _ in 0..count {
            let (node_id, _runner) = self.add_node(rng).await.unwrap();
            node_ids.push(node_id);
        }
        node_ids
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
    pub async fn add_node_with_config<RNG: Rng + ?Sized>(
        &mut self,
        cfg: R::Config,
        rng: &mut RNG,
    ) -> Result<(R::NodeId, &mut Runner<ConditionCheckReactor<R>>), R::Error> {
        let runner: Runner<ConditionCheckReactor<R>> = Runner::new(cfg, rng).await?;

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

    /// Removes a node from the network.
    pub fn remove_node(&mut self, node_id: &R::NodeId) -> Option<Runner<ConditionCheckReactor<R>>> {
        self.nodes.remove(node_id)
    }

    /// Crank the specified runner once, returning the number of events processed.
    pub async fn crank<RNG: Rng + ?Sized>(&mut self, node_id: &R::NodeId, rng: &mut RNG) -> usize {
        let runner = self.nodes.get_mut(node_id).expect("should find node");

        let node_id = runner.reactor().node_id();
        let span = tracing::error_span!("crank", node_id = %node_id);
        if runner.try_crank(rng).instrument(span).await.is_some() {
            1
        } else {
            0
        }
    }

    /// Crank only the specified runner until `condition` is true or until `within` has elapsed.
    ///
    /// Returns `true` if `condition` has been met within the specified timeout.
    pub async fn crank_until<RNG, F>(
        &mut self,
        node_id: &R::NodeId,
        rng: &mut RNG,
        condition: F,
        within: Duration,
    ) where
        RNG: Rng + ?Sized,
        F: Fn(&R::Event) -> bool + Send + 'static,
    {
        self.nodes
            .get_mut(node_id)
            .unwrap()
            .reactor_mut()
            .set_condition_checker(Box::new(condition));

        time::timeout(within, self.crank_and_check_indefinitely(node_id, rng))
            .await
            .unwrap()
    }

    async fn crank_and_check_indefinitely<RNG: Rng + ?Sized>(
        &mut self,
        node_id: &R::NodeId,
        rng: &mut RNG,
    ) {
        loop {
            if self.crank(node_id, rng).await == 0 {
                time::delay_for(POLL_INTERVAL).await;
                continue;
            }

            if self
                .nodes
                .get(node_id)
                .unwrap()
                .reactor()
                .condition_result()
            {
                debug!("{} met condition", node_id);
                return;
            }
        }
    }

    /// Crank all runners once, returning the number of events processed.
    pub async fn crank_all<RNG: Rng + ?Sized>(&mut self, rng: &mut RNG) -> usize {
        let mut event_count = 0;
        for node in self.nodes.values_mut() {
            let node_id = node.reactor().node_id();
            let span = tracing::error_span!("crank", node_id = %node_id);
            event_count += if node.try_crank(rng).instrument(span).await.is_some() {
                1
            } else {
                0
            }
        }

        event_count
    }

    /// Process events on all nodes until all event queues are empty for at least `quiet_for`.
    ///
    /// # Panics
    ///
    /// Panics if after `within` the event queues are still not idle.
    pub async fn settle<RNG: Rng + ?Sized>(
        &mut self,
        rng: &mut RNG,
        quiet_for: Duration,
        within: Duration,
    ) {
        time::timeout(within, self.settle_indefinitely(rng, quiet_for))
            .await
            .unwrap_or_else(|_| {
                panic!(format!(
                    "network did not settle for {:?} within {:?}",
                    quiet_for, within
                ))
            })
    }

    async fn settle_indefinitely<RNG: Rng + ?Sized>(&mut self, rng: &mut RNG, quiet_for: Duration) {
        let mut no_events = false;
        loop {
            if self.crank_all(rng).await == 0 {
                // Stop once we have no pending events and haven't had any for `quiet_for` time.
                if no_events {
                    debug!("network has been quiet for {:?}", quiet_for);
                    break;
                } else {
                    no_events = true;
                    time::delay_for(quiet_for).await;
                }
            } else {
                no_events = false;
            }
        }
    }

    /// Runs the main loop of every reactor until `condition` is true.
    ///
    /// # Panics
    ///
    /// If the `condition` is not reached inside of `within`, panics.
    pub async fn settle_on<RNG, F>(&mut self, rng: &mut RNG, condition: F, within: Duration)
    where
        RNG: Rng + ?Sized,
        F: Fn(&HashMap<R::NodeId, Runner<ConditionCheckReactor<R>>>) -> bool,
    {
        time::timeout(within, self.settle_on_indefinitely(rng, condition))
            .await
            .unwrap_or_else(|_| {
                panic!(format!(
                    "network did not settle on condition within {:?}",
                    within
                ))
            })
    }

    pub async fn settle_on_indefinitely<RNG, F>(&mut self, rng: &mut RNG, condition: F)
    where
        RNG: Rng + ?Sized,
        F: Fn(&HashMap<R::NodeId, Runner<ConditionCheckReactor<R>>>) -> bool,
    {
        loop {
            if condition(&self.nodes) {
                debug!("network settled on meeting condition");
                break;
            }

            if self.crank_all(rng).await == 0 {
                // No events processed, wait for a bit to avoid 100% cpu usage.
                time::delay_for(POLL_INTERVAL).await;
            }
        }
    }

    /// Returns the internal map of nodes.
    pub fn nodes(&self) -> &HashMap<R::NodeId, Runner<ConditionCheckReactor<R>>> {
        &self.nodes
    }

    /// Create effects and dispatch them on the given node.
    ///
    /// The effects are created via a call to `create_effects` which is itself passed an instance of
    /// an `EffectBuilder`.
    pub async fn process_injected_effect_on<F>(&mut self, node_id: &R::NodeId, create_effects: F)
    where
        F: FnOnce(EffectBuilder<R::Event>) -> Effects<R::Event>,
    {
        let runner = self.nodes.get_mut(node_id).unwrap();
        let node_id = runner.reactor().node_id();
        let span = tracing::error_span!("inject", node_id = %node_id);
        runner
            .process_injected_effects(create_effects)
            .instrument(span)
            .await
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
