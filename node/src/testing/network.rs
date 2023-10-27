//! A network of test reactors.

use std::{
    collections::{hash_map::Entry, HashMap},
    fmt::Debug,
    mem,
    sync::Arc,
    time::Duration,
};

use fake_instant::FakeClock as Instant;
use futures::future::{BoxFuture, FutureExt};
use serde::Serialize;
use tokio::time::{self, error::Elapsed};
use tracing::{debug, error_span};
use tracing_futures::Instrument;

use casper_types::testing::TestRng;

use super::ConditionCheckReactor;
use crate::{
    effect::{EffectBuilder, Effects},
    reactor::{Finalize, Reactor, Runner, TryCrankOutcome},
    tls::KeyFingerprint,
    types::{Chainspec, ChainspecRawBytes, ExitCode, NodeId},
    utils::Loadable,
    NodeRng,
};

/// Type alias for set of nodes inside a network.
///
/// Provided as a convenience for writing condition functions for `settle_on` and friends.
pub(crate) type Nodes<R> = HashMap<NodeId, Runner<ConditionCheckReactor<R>>>;

/// A reactor with networking functionality.
///
/// Test reactors implementing this SHOULD implement at least the `node_id` function if they have
/// proper networking functionality.
pub(crate) trait NetworkedReactor: Sized {
    /// Returns the node ID assigned to this specific reactor instance.
    ///
    /// The default implementation generates a pseudo-id base on its memory address.
    fn node_id(&self) -> NodeId {
        #[allow(trivial_casts)]
        let addr = self as *const _ as usize;
        let mut raw: [u8; KeyFingerprint::LENGTH] = [0; KeyFingerprint::LENGTH];
        raw[0..(mem::size_of::<usize>())].copy_from_slice(&addr.to_be_bytes());
        NodeId::from(KeyFingerprint::from(raw))
    }
}

/// Time interval for which to poll an observed testing network when no events have occurred.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// A network of multiple test reactors.
///
/// Nodes themselves are not run in the background, rather manual cranking is required through
/// `crank_all`. As an alternative, the `settle` and `settle_all` functions can be used to continue
/// cranking until a condition has been reached.
#[derive(Debug, Default)]
pub(crate) struct TestingNetwork<R: Reactor + NetworkedReactor> {
    /// Current network.
    nodes: HashMap<NodeId, Runner<ConditionCheckReactor<R>>>,
}

impl<R> TestingNetwork<R>
where
    R: Reactor + NetworkedReactor,
    R::Config: Default,
    <R as Reactor>::Error: Debug,
    R::Event: Serialize,
    R::Error: From<prometheus::Error>,
{
    /// Creates a new networking node on the network using the default root node port.
    ///
    /// # Panics
    ///
    /// Panics if a duplicate node ID is being inserted. This should only happen in case a randomly
    /// generated ID collides.
    pub(crate) async fn add_node<'a, 'b: 'a>(
        &'a mut self,
        rng: &'b mut TestRng,
    ) -> Result<(NodeId, &'_ mut Runner<ConditionCheckReactor<R>>), R::Error> {
        self.add_node_with_config(Default::default(), rng).await
    }

    /// Adds `count` new nodes to the network, and returns their IDs.
    pub(crate) async fn add_nodes(&mut self, rng: &mut TestRng, count: usize) -> Vec<NodeId> {
        let mut node_ids = vec![];
        for _ in 0..count {
            let (node_id, _runner) = self.add_node(rng).await.unwrap();
            node_ids.push(node_id);
        }
        node_ids
    }
}

impl<R> TestingNetwork<R>
where
    R: Reactor + NetworkedReactor,
    R::Event: Serialize,
    R::Error: From<prometheus::Error> + From<R::Error>,
{
    /// Creates a new network.
    pub(crate) fn new() -> Self {
        TestingNetwork {
            nodes: HashMap::new(),
        }
    }

    /// Creates a new networking node on the network.
    ///
    /// # Panics
    ///
    /// Panics if a duplicate node ID is being inserted.
    pub(crate) async fn add_node_with_config<'a, 'b: 'a>(
        &'a mut self,
        cfg: R::Config,
        rng: &'b mut NodeRng,
    ) -> Result<(NodeId, &mut Runner<ConditionCheckReactor<R>>), R::Error> {
        let (chainspec, chainspec_raw_bytes) =
            <(Chainspec, ChainspecRawBytes)>::from_resources("local");
        self.add_node_with_config_and_chainspec(
            cfg,
            Arc::new(chainspec),
            Arc::new(chainspec_raw_bytes),
            rng,
        )
        .await
    }

    /// Creates a new networking node on the network.
    ///
    /// # Panics
    ///
    /// Panics if a duplicate node ID is being inserted.
    pub(crate) async fn add_node_with_config_and_chainspec<'a, 'b: 'a>(
        &'a mut self,
        cfg: R::Config,
        chainspec: Arc<Chainspec>,
        chainspec_raw_bytes: Arc<ChainspecRawBytes>,
        rng: &'b mut NodeRng,
    ) -> Result<(NodeId, &mut Runner<ConditionCheckReactor<R>>), R::Error> {
        let runner: Runner<ConditionCheckReactor<R>> =
            Runner::new(cfg, chainspec, chainspec_raw_bytes, rng).await?;

        let node_id = runner.reactor().node_id();

        let node_ref = match self.nodes.entry(node_id) {
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
    pub(crate) fn remove_node(
        &mut self,
        node_id: &NodeId,
    ) -> Option<Runner<ConditionCheckReactor<R>>> {
        self.nodes.remove(node_id)
    }

    /// Crank the specified runner once.
    pub(crate) async fn crank(&mut self, node_id: &NodeId, rng: &mut TestRng) -> TryCrankOutcome {
        let runner = self.nodes.get_mut(node_id).expect("should find node");
        let node_id = runner.reactor().node_id();
        runner
            .try_crank(rng)
            .instrument(error_span!("crank", node_id = %node_id))
            .await
    }

    /// Crank only the specified runner until `condition` is true or until `within` has elapsed.
    ///
    /// Returns `true` if `condition` has been met within the specified timeout.
    ///
    /// Panics if cranking causes the node to return an exit code.
    pub(crate) async fn crank_until<F>(
        &mut self,
        node_id: &NodeId,
        rng: &mut TestRng,
        condition: F,
        within: Duration,
    ) where
        F: Fn(&R::Event) -> bool + Send + 'static,
    {
        self.nodes
            .get_mut(node_id)
            .unwrap()
            .crank_until(rng, condition, within)
            .await
    }

    /// Crank all runners once, returning the number of events processed.
    ///
    /// Panics if any node returns an exit code.
    async fn crank_all(&mut self, rng: &mut TestRng) -> usize {
        let mut event_count = 0;
        for node in self.nodes.values_mut() {
            let node_id = node.reactor().node_id();
            match node
                .try_crank(rng)
                .instrument(error_span!("crank", node_id = %node_id))
                .await
            {
                TryCrankOutcome::NoEventsToProcess => (),
                TryCrankOutcome::ProcessedAnEvent => event_count += 1,
                TryCrankOutcome::ShouldExit(exit_code) => {
                    panic!("should not exit: {:?}", exit_code)
                }
                TryCrankOutcome::Exited => unreachable!(),
            }
        }

        event_count
    }

    /// Crank all runners until `condition` is true on the specified runner or until `within` has
    /// elapsed.
    ///
    /// Returns `true` if `condition` has been met within the specified timeout.
    ///
    /// Panics if cranking causes the node to return an exit code.
    pub(crate) async fn crank_all_until<F>(
        &mut self,
        node_id: &NodeId,
        rng: &mut TestRng,
        condition: F,
        within: Duration,
    ) where
        F: Fn(&R::Event) -> bool + Send + 'static,
    {
        self.nodes
            .get_mut(node_id)
            .unwrap()
            .reactor_mut()
            .set_condition_checker(Box::new(condition));

        time::timeout(within, self.crank_and_check_all_indefinitely(node_id, rng))
            .await
            .unwrap()
    }

    async fn crank_and_check_all_indefinitely(
        &mut self,
        node_to_check: &NodeId,
        rng: &mut TestRng,
    ) {
        loop {
            let mut no_events = true;
            for node in self.nodes.values_mut() {
                let node_id = node.reactor().node_id();
                match node
                    .try_crank(rng)
                    .instrument(error_span!("crank", node_id = %node_id))
                    .await
                {
                    TryCrankOutcome::NoEventsToProcess => (),
                    TryCrankOutcome::ProcessedAnEvent => {
                        no_events = false;
                    }
                    TryCrankOutcome::ShouldExit(exit_code) => {
                        panic!("should not exit: {:?}", exit_code)
                    }
                    TryCrankOutcome::Exited => unreachable!(),
                }
                if node_id == *node_to_check && node.reactor().condition_result() {
                    debug!("{} met condition", node_to_check);
                    return;
                }
            }

            if no_events {
                Instant::advance_time(POLL_INTERVAL.as_millis() as u64);
                time::sleep(POLL_INTERVAL).await;
                continue;
            }
        }
    }

    /// Process events on all nodes until all event queues are empty for at least `quiet_for`.
    ///
    /// Panics if after `within` the event queues are still not idle, or if any node returns an exit
    /// code.
    pub(crate) async fn settle(
        &mut self,
        rng: &mut TestRng,
        quiet_for: Duration,
        within: Duration,
    ) {
        time::timeout(within, self.settle_indefinitely(rng, quiet_for))
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "network did not settle for {:?} within {:?}",
                    quiet_for, within
                )
            })
    }

    async fn settle_indefinitely(&mut self, rng: &mut TestRng, quiet_for: Duration) {
        let mut no_events = false;
        loop {
            if self.crank_all(rng).await == 0 {
                // Stop once we have no pending events and haven't had any for `quiet_for` time.
                if no_events {
                    debug!("network has been quiet for {:?}", quiet_for);
                    break;
                } else {
                    no_events = true;
                    Instant::advance_time(quiet_for.as_millis() as u64);
                    time::sleep(quiet_for).await;
                }
            } else {
                no_events = false;
            }
        }
    }

    /// Runs the main loop of every reactor until `condition` is true.
    ///
    /// Returns an error if the `condition` is not reached inside of `within`.
    ///
    /// Panics if any node returns an exit code.  To settle on an exit code, use `settle_on_exit`
    /// instead.
    pub(crate) async fn try_settle_on<F>(
        &mut self,
        rng: &mut TestRng,
        condition: F,
        within: Duration,
    ) -> Result<(), Elapsed>
    where
        F: Fn(&Nodes<R>) -> bool,
    {
        time::timeout(within, self.settle_on_indefinitely(rng, condition)).await
    }

    /// Runs the main loop of every reactor until `condition` is true.
    ///
    /// Panics if the `condition` is not reached inside of `within`, or if any node returns an exit
    /// code.
    ///
    /// To settle on an exit code, use `settle_on_exit` instead.
    pub(crate) async fn settle_on<F>(&mut self, rng: &mut TestRng, condition: F, within: Duration)
    where
        F: Fn(&Nodes<R>) -> bool,
    {
        self.try_settle_on(rng, condition, within)
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "network did not settle on condition within {} seconds",
                    within.as_secs_f64()
                )
            })
    }

    async fn settle_on_indefinitely<F>(&mut self, rng: &mut TestRng, condition: F)
    where
        F: Fn(&Nodes<R>) -> bool,
    {
        loop {
            if condition(&self.nodes) {
                debug!("network settled on meeting condition");
                break;
            }

            if self.crank_all(rng).await == 0 {
                // No events processed, wait for a bit to avoid 100% cpu usage.
                Instant::advance_time(POLL_INTERVAL.as_millis() as u64);
                time::sleep(POLL_INTERVAL).await;
            }
        }
    }

    /// Runs the main loop of every reactor until the nodes return the expected exit code.
    ///
    /// Panics if the nodes do not exit inside of `within`, or if any node returns an unexpected
    /// exit code.
    pub(crate) async fn settle_on_exit(
        &mut self,
        rng: &mut TestRng,
        expected: ExitCode,
        within: Duration,
    ) {
        time::timeout(within, self.settle_on_exit_indefinitely(rng, expected))
            .await
            .unwrap_or_else(|_| panic!("network did not settle on condition within {:?}", within))
    }

    async fn settle_on_exit_indefinitely(&mut self, rng: &mut TestRng, expected: ExitCode) {
        let mut exited_as_expected = 0;
        loop {
            if exited_as_expected == self.nodes.len() {
                debug!(?expected, "all nodes exited with expected code");
                break;
            }

            let mut event_count = 0;
            for node in self.nodes.values_mut() {
                let node_id = node.reactor().node_id();
                match node
                    .try_crank(rng)
                    .instrument(error_span!("crank", node_id = %node_id))
                    .await
                {
                    TryCrankOutcome::NoEventsToProcess => (),
                    TryCrankOutcome::ProcessedAnEvent => event_count += 1,
                    TryCrankOutcome::ShouldExit(exit_code) if exit_code == expected => {
                        exited_as_expected += 1;
                        event_count += 1;
                    }
                    TryCrankOutcome::ShouldExit(exit_code) => {
                        panic!(
                            "unexpected exit: expected {:?}, got {:?}",
                            expected, exit_code
                        )
                    }
                    TryCrankOutcome::Exited => (),
                }
            }

            if event_count == 0 {
                // No events processed, wait for a bit to avoid 100% cpu usage.
                Instant::advance_time(POLL_INTERVAL.as_millis() as u64);
                time::sleep(POLL_INTERVAL).await;
            }
        }
    }

    /// Returns the internal map of nodes.
    pub(crate) fn nodes(&self) -> &HashMap<NodeId, Runner<ConditionCheckReactor<R>>> {
        &self.nodes
    }

    /// Returns the internal map of nodes, mutable.
    pub(crate) fn nodes_mut(&mut self) -> &mut HashMap<NodeId, Runner<ConditionCheckReactor<R>>> {
        &mut self.nodes
    }

    /// Returns an iterator over all runners, mutable.
    pub(crate) fn runners_mut(
        &mut self,
    ) -> impl Iterator<Item = &mut Runner<ConditionCheckReactor<R>>> {
        self.nodes.values_mut()
    }

    /// Returns an iterator over all reactors, mutable.
    pub(crate) fn reactors_mut(&mut self) -> impl Iterator<Item = &mut R> {
        self.runners_mut()
            .map(|runner| runner.reactor_mut().inner_mut())
    }

    /// Create effects and dispatch them on the given node.
    ///
    /// The effects are created via a call to `create_effects` which is itself passed an instance of
    /// an `EffectBuilder`.
    pub(crate) async fn process_injected_effect_on<F>(
        &mut self,
        node_id: &NodeId,
        create_effects: F,
    ) where
        F: FnOnce(EffectBuilder<R::Event>) -> Effects<R::Event>,
    {
        let runner = self.nodes.get_mut(node_id).unwrap();
        let node_id = runner.reactor().node_id();
        runner
            .process_injected_effects(create_effects)
            .instrument(error_span!("inject", node_id = %node_id))
            .await
    }
}

impl<R> Finalize for TestingNetwork<R>
where
    R: Finalize + NetworkedReactor + Reactor + Send + 'static,
    R::Event: Serialize + Send + Sync,
    R::Error: From<prometheus::Error>,
{
    fn finalize(self) -> BoxFuture<'static, ()> {
        // We support finalizing networks where the reactor itself can be finalized.

        async move {
            // Shutdown the sender of every reactor node to ensure the port is open again.
            for (_, node) in self.nodes.into_iter() {
                node.drain_into_inner().await.finalize().await;
            }

            debug!("network finalized");
        }
        .boxed()
    }
}
