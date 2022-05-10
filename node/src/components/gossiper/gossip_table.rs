#[cfg(not(test))]
use std::time::Instant;
use std::{
    collections::{HashMap, HashSet},
    fmt::{self, Display, Formatter},
    hash::Hash,
    time::Duration,
};

use datasize::DataSize;
#[cfg(test)]
use fake_instant::FakeClock as Instant;
use tracing::{debug, error, warn};

use super::Config;
use crate::{types::NodeId, utils::DisplayIter};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum GossipAction {
    /// This is new data, previously unknown by us, and for which we don't yet hold everything
    /// required to allow us start gossiping it onwards.  We should get the remaining parts from
    /// the provided holder and not gossip the ID onwards yet.
    GetRemainder { holder: NodeId },
    /// This is data already known to us, but for which we don't yet hold everything required to
    /// allow us start gossiping it onwards.  We should already be getting the remaining parts from
    /// a holder, so there's no need to do anything else now.
    AwaitingRemainder,
    /// We hold the data locally and should gossip the ID onwards.
    ShouldGossip(ShouldGossip),
    /// We hold the data locally, and we shouldn't gossip the ID onwards.
    Noop,
    /// We just finished gossiping the data: no need to gossip further, but an announcement that we
    /// have finished gossiping this data should be made.
    AnnounceFinished,
}

impl Display for GossipAction {
    fn fmt(&self, formatter: &mut Formatter) -> Result<(), fmt::Error> {
        match self {
            GossipAction::GetRemainder { holder } => {
                write!(formatter, "should get remainder from {}", holder)
            }
            GossipAction::AwaitingRemainder => write!(formatter, "awaiting remainder"),
            GossipAction::ShouldGossip(should_gossip) => Display::fmt(should_gossip, formatter),
            GossipAction::Noop => write!(formatter, "should do nothing"),
            GossipAction::AnnounceFinished => write!(formatter, "finished gossiping"),
        }
    }
}

/// Used as a return type from API methods to indicate that the caller should continue to gossip the
/// given data.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ShouldGossip {
    /// The number of copies of the gossip message to send.
    pub(crate) count: usize,
    /// Peers we should avoid gossiping this data to, since they already hold it.
    pub(crate) exclude_peers: HashSet<NodeId>,
    /// Whether we already held the full data or not.
    pub(crate) is_already_held: bool,
}

impl Display for ShouldGossip {
    fn fmt(&self, formatter: &mut Formatter) -> Result<(), fmt::Error> {
        write!(formatter, "should gossip to {} peer(s) ", self.count)?;
        if !self.exclude_peers.is_empty() {
            write!(
                formatter,
                "excluding {} ",
                DisplayIter::new(&self.exclude_peers)
            )?;
        }
        write!(
            formatter,
            "(we {} the item)",
            if self.is_already_held {
                "previously held"
            } else {
                "didn't previously hold"
            }
        )
    }
}

#[derive(DataSize, Debug, Default)]
pub(crate) struct State {
    /// The peers excluding us which hold the data.
    holders: HashSet<NodeId>,
    /// Whether we hold the full data locally yet or not.
    held_by_us: bool,
    /// The subset of `holders` we have infected.  Not just a count so we don't attribute the same
    /// peer multiple times.
    infected_by_us: HashSet<NodeId>,
    /// The count of in-flight gossip messages sent by us for this data.
    in_flight_count: usize,
}

impl State {
    /// Returns whether we should finish gossiping this data.
    fn is_finished(&self, infection_target: usize, holders_limit: usize) -> bool {
        self.infected_by_us.len() >= infection_target || self.holders.len() >= holders_limit
    }

    /// Returns a `GossipAction` derived from the given state.
    fn action(
        &mut self,
        infection_target: usize,
        holders_limit: usize,
        is_new: bool,
    ) -> GossipAction {
        if self.is_finished(infection_target, holders_limit) {
            return GossipAction::Noop;
        }

        if self.held_by_us {
            let count =
                infection_target.saturating_sub(self.in_flight_count + self.infected_by_us.len());
            if count > 0 {
                self.in_flight_count += count;
                return GossipAction::ShouldGossip(ShouldGossip {
                    count,
                    exclude_peers: self.holders.clone(),
                    is_already_held: !is_new,
                });
            } else {
                return GossipAction::Noop;
            }
        }

        if is_new {
            let holder = *self
                .holders
                .iter()
                .next()
                .expect("holders cannot be empty if we don't hold the data");
            GossipAction::GetRemainder { holder }
        } else {
            GossipAction::AwaitingRemainder
        }
    }
}

#[derive(DataSize, Debug)]
pub(crate) struct Timeouts<T> {
    values: Vec<(Instant, T)>,
}

impl<T> Timeouts<T> {
    fn new() -> Self {
        Timeouts { values: Vec::new() }
    }

    fn push(&mut self, timeout: Instant, data_id: T) {
        self.values.push((timeout, data_id));
    }

    fn purge(&mut self, now: &Instant) -> impl Iterator<Item = T> + '_ {
        // The values are sorted by timeout.  Locate the index of the first non-expired one.
        let split_index = match self
            .values
            .binary_search_by(|(timeout, _data_id)| timeout.cmp(now))
        {
            Ok(index) => index,
            Err(index) => index,
        };

        // Drain and return the expired IDs.
        self.values
            .drain(..split_index)
            .map(|(_timeout, data_id)| data_id)
    }
}

#[derive(DataSize, Debug)]
pub(crate) struct GossipTable<T> {
    /// Data IDs for which gossiping is still ongoing.
    current: HashMap<T, State>,
    /// Data IDs for which gossiping is complete.
    finished: HashSet<T>,
    /// Timeouts for removal of items from the `finished` cache.
    timeouts: Timeouts<T>,
    /// See `Config::infection_target`.
    infection_target: usize,
    /// Derived from `Config::saturation_limit_percent` - we gossip data while the number of
    /// holders doesn't exceed `holders_limit`.
    holders_limit: usize,
    /// See `Config::finished_entry_duration`.
    finished_entry_duration: Duration,
}

impl<T> GossipTable<T> {
    /// Number of items currently being gossiped.
    pub fn items_current(&self) -> usize {
        self.current.len()
    }

    /// Number of items that are kept but are finished gossiping.
    pub fn items_finished(&self) -> usize {
        self.finished.len()
    }
}

impl<T: Copy + Eq + Hash + Display> GossipTable<T> {
    /// Returns a new `GossipTable` using the provided configuration.
    pub(crate) fn new(config: Config) -> Self {
        let holders_limit = (100 * usize::from(config.infection_target()))
            / (100 - usize::from(config.saturation_limit_percent()));
        GossipTable {
            current: HashMap::new(),
            finished: HashSet::new(),
            timeouts: Timeouts::new(),
            infection_target: usize::from(config.infection_target()),
            holders_limit,
            finished_entry_duration: config.finished_entry_duration().into(),
        }
    }

    /// We received knowledge about potentially new data with given ID from the given peer.  This
    /// should only be called where we don't already hold everything locally we need to be able to
    /// gossip it onwards.  If we are able to gossip the data already, call `new_data` instead.
    ///
    /// Once we have retrieved everything we need in order to begin gossiping onwards, call
    /// `new_data`.
    ///
    /// Returns whether we should gossip it, and a list of peers to exclude.
    pub(crate) fn new_partial_data(&mut self, data_id: &T, holder: NodeId) -> GossipAction {
        self.purge_finished();

        if self.finished.contains(data_id) {
            debug!(item=%data_id, "no further action: item already finished");
            return GossipAction::Noop;
        }

        let update = |state: &mut State| {
            let _ = state.holders.insert(holder);
        };

        if let Some(action) = self.update_current(data_id, update) {
            debug!(item=%data_id, %action, "item is currently being gossiped");
            return action;
        }

        // This isn't in finished or current - add a new entry to current.
        let mut state = State::default();
        update(&mut state);
        let is_new = true;
        let action = state.action(self.infection_target, self.holders_limit, is_new);
        let _ = self.current.insert(*data_id, state);
        debug!(item=%data_id, %action, "gossiping new item should begin");
        action
    }

    /// We received or generated potentially new data with given ID.  If received from a peer,
    /// its ID should be passed in `maybe_holder`.  If received from a client or generated on this
    /// node, `maybe_holder` should be `None`.
    ///
    /// This should only be called once we hold everything locally we need to be able to gossip it
    /// onwards.  If we aren't able to gossip this data yet, call `new_partial_data` instead.
    ///
    /// Returns whether we should gossip it, and a list of peers to exclude.
    pub(crate) fn new_complete_data(
        &mut self,
        data_id: &T,
        maybe_holder: Option<NodeId>,
    ) -> GossipAction {
        self.purge_finished();

        if self.finished.contains(data_id) {
            debug!(item=%data_id, "no further action: item already finished");
            return GossipAction::Noop;
        }

        let update = |state: &mut State| {
            state.holders.extend(maybe_holder);
            state.held_by_us = true;
        };

        if let Some(action) = self.update_current(data_id, update) {
            debug!(item=%data_id, %action, "item is currently being gossiped");
            return action;
        }

        // This isn't in finished or current - add a new entry to current.
        let mut state = State::default();
        update(&mut state);
        let is_new = true;
        let action = state.action(self.infection_target, self.holders_limit, is_new);
        let _ = self.current.insert(*data_id, state);
        debug!(item=%data_id, %action, "gossiping new item should begin");
        action
    }

    /// We got a response from a peer we gossiped to indicating we infected it (it didn't previously
    /// know of this data).
    ///
    /// If the given `data_id` is not a member of the current entries (those not deemed finished),
    /// then `GossipAction::Noop` will be returned under the assumption that the data has already
    /// finished being gossiped.
    pub(crate) fn we_infected(&mut self, data_id: &T, peer: NodeId) -> GossipAction {
        let infected_by_us = true;
        self.infected(data_id, peer, infected_by_us)
    }

    /// We got a response from a peer we gossiped to indicating it was already infected (it
    /// previously knew of this data).
    ///
    /// If the given `data_id` is not a member of the current entries (those not deemed finished),
    /// then `GossipAction::Noop` will be returned under the assumption that the data has already
    /// finished being gossiped.
    pub(crate) fn already_infected(&mut self, data_id: &T, peer: NodeId) -> GossipAction {
        let infected_by_us = false;
        self.infected(data_id, peer, infected_by_us)
    }

    fn infected(&mut self, data_id: &T, peer: NodeId, by_us: bool) -> GossipAction {
        let update = |state: &mut State| {
            if !state.held_by_us {
                warn!(
                    item=%data_id,
                    %peer, "shouldn't have received a gossip response for partial data"
                );
                return;
            }
            let _ = state.holders.insert(peer);
            if by_us {
                let _ = state.infected_by_us.insert(peer);
            }
            state.in_flight_count = state.in_flight_count.saturating_sub(1);
        };

        self.update_current(data_id, update)
            .unwrap_or(GossipAction::Noop)
    }

    /// Directly reduces the in-flight count of gossip requests for the given item by the given
    /// amount.
    ///
    /// Returns `true` if there was a current entry for this data and it is now finished.
    ///
    /// This should be called if, after trying to gossip to a given number of peers, we find that
    /// we've not been able to select enough peers.  Without this reduction, the given gossip item
    /// would never move from `current` to `finished`, and hence would never be purged.
    pub(crate) fn reduce_in_flight_count(&mut self, data_id: &T, reduce_by: usize) -> bool {
        let should_finish = if let Some(state) = self.current.get_mut(data_id) {
            state.in_flight_count = state.in_flight_count.saturating_sub(reduce_by);
            debug!(
                item=%data_id,
                in_flight_count=%state.in_flight_count,
                "reduced in-flight count for item"
            );
            state.in_flight_count == 0
        } else {
            false
        };

        if should_finish {
            debug!(item=%data_id, "finished gossiping since no more peers to gossip to");
            return self.force_finish(data_id);
        }

        false
    }

    /// Checks if gossip request we sent timed out.
    ///
    /// If the peer is already counted as a holder, it has previously responded and this method
    /// returns Noop.  Otherwise it has timed out and we return the appropriate action to take.
    pub(crate) fn check_timeout(&mut self, data_id: &T, peer: NodeId) -> GossipAction {
        let update = |state: &mut State| {
            debug_assert!(
                state.held_by_us,
                "shouldn't check timeout for a gossip response for partial data"
            );
            if !state.held_by_us {
                error!(
                    item=%data_id,
                    %peer, "shouldn't check timeout for a gossip response for partial data"
                );
                return;
            }

            if !state.holders.contains(&peer) {
                // Add the peer as a holder just to avoid retrying it.
                let _ = state.holders.insert(peer);
                state.in_flight_count = state.in_flight_count.saturating_sub(1);
            }
        };

        self.update_current(data_id, update)
            .unwrap_or(GossipAction::Noop)
    }

    /// If we hold the full data, assume `peer` provided it to us and shouldn't be removed as a
    /// holder.  Otherwise, assume `peer` was unresponsive and remove from list of holders.
    ///
    /// If this causes the list of holders to become empty, and we also don't hold the full data,
    /// then this entry is removed as if we'd never heard of it.
    pub(crate) fn remove_holder_if_unresponsive(
        &mut self,
        data_id: &T,
        peer: NodeId,
    ) -> GossipAction {
        if let Some(mut state) = self.current.remove(data_id) {
            if !state.held_by_us {
                let _ = state.holders.remove(&peer);
                debug!(item=%data_id, %peer, "removed peer as a holder of the item");
                if state.holders.is_empty() {
                    // We don't hold the full data, and we don't know any holders - remove the entry
                    debug!(item=%data_id, "no further action: item now removed as no holders");
                    return GossipAction::Noop;
                }
            }
            let is_new = !state.held_by_us;
            let action = state.action(self.infection_target, self.holders_limit, is_new);
            let _ = self.current.insert(*data_id, state);
            debug!(item=%data_id, %action, "assuming peer response did not timeout");
            return action;
        }

        GossipAction::Noop
    }

    /// We have deemed the data not suitable for gossiping further.  The entry will be marked as
    /// `finished` and eventually be purged.
    ///
    /// Returns `true` if there was a current entry for this data.
    pub(crate) fn force_finish(&mut self, data_id: &T) -> bool {
        if self.current.remove(data_id).is_some() {
            self.insert_to_finished(data_id);
            return true;
        }
        false
    }

    /// Updates the entry under `data_id` in `self.current` and returns the action we should now
    /// take, or `None` if the entry does not exist.
    ///
    /// If the entry becomes finished, it is moved from `self.current` to `self.finished`.
    fn update_current<F: Fn(&mut State)>(
        &mut self,
        data_id: &T,
        update: F,
    ) -> Option<GossipAction> {
        let mut state = self.current.remove(data_id)?;
        update(&mut state);
        if state.is_finished(self.infection_target, self.holders_limit) {
            self.insert_to_finished(data_id);
            return Some(GossipAction::AnnounceFinished);
        }
        let is_new = false;
        let action = state.action(self.infection_target, self.holders_limit, is_new);
        let _ = self.current.insert(*data_id, state);
        Some(action)
    }

    fn insert_to_finished(&mut self, data_id: &T) {
        let timeout = Instant::now() + self.finished_entry_duration;
        let _ = self.finished.insert(*data_id);
        let _ = self.timeouts.push(timeout, *data_id);
    }

    /// Retains only those finished entries which still haven't timed out.
    fn purge_finished(&mut self) {
        let now = Instant::now();

        for expired_finished in self.timeouts.purge(&now) {
            let _ = self.finished.remove(&expired_finished);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, iter, str::FromStr};

    use casper_types::{testing::TestRng, TimeDiff};
    use rand::Rng;

    use super::{super::config::DEFAULT_FINISHED_ENTRY_DURATION, *};
    use crate::{logging, utils::DisplayIter};

    const EXPECTED_DEFAULT_INFECTION_TARGET: usize = 3;
    const EXPECTED_DEFAULT_HOLDERS_LIMIT: usize = 15;

    fn random_node_ids(rng: &mut TestRng) -> Vec<NodeId> {
        iter::repeat_with(|| NodeId::random(rng))
            .take(EXPECTED_DEFAULT_HOLDERS_LIMIT + 3)
            .collect()
    }

    fn check_holders(expected: &[NodeId], gossip_table: &GossipTable<u64>, data_id: &u64) {
        let expected: BTreeSet<_> = expected.iter().collect();
        let actual: BTreeSet<_> = gossip_table
            .current
            .get(data_id)
            .map_or_else(BTreeSet::new, |state| state.holders.iter().collect());
        assert!(
            expected == actual,
            "\nexpected: {}\nactual:   {}\n",
            DisplayIter::new(expected.iter()),
            DisplayIter::new(actual.iter())
        );
    }

    #[test]
    fn new_partial_data() {
        let _ = logging::init();
        let mut rng = crate::new_rng();
        let node_ids = random_node_ids(&mut rng);
        let data_id: u64 = rng.gen();

        let mut gossip_table = GossipTable::new(Config::default());
        assert_eq!(
            EXPECTED_DEFAULT_INFECTION_TARGET,
            gossip_table.infection_target
        );
        assert_eq!(EXPECTED_DEFAULT_HOLDERS_LIMIT, gossip_table.holders_limit);

        // Check new partial data causes `GetRemainder` to be returned.
        let action = gossip_table.new_partial_data(&data_id, node_ids[0]);
        let expected = GossipAction::GetRemainder {
            holder: node_ids[0],
        };
        assert_eq!(expected, action);
        check_holders(&node_ids[..1], &gossip_table, &data_id);

        // Check same partial data from same source causes `AwaitingRemainder` to be returned.
        let action = gossip_table.new_partial_data(&data_id, node_ids[0]);
        assert_eq!(GossipAction::AwaitingRemainder, action);
        check_holders(&node_ids[..1], &gossip_table, &data_id);

        // Check same partial data from different source causes `AwaitingRemainder` to be returned
        // and holders updated.
        let action = gossip_table.new_partial_data(&data_id, node_ids[1]);
        assert_eq!(GossipAction::AwaitingRemainder, action);
        check_holders(&node_ids[..2], &gossip_table, &data_id);

        // Finish the gossip by reporting three infections, then check same partial data causes
        // `Noop` to be returned and holders cleared.
        let _ = gossip_table.new_complete_data(&data_id, Some(node_ids[0]));
        let limit = 3 + EXPECTED_DEFAULT_INFECTION_TARGET;
        for node_id in &node_ids[3..limit] {
            let _ = gossip_table.we_infected(&data_id, *node_id);
        }
        let action = gossip_table.new_partial_data(&data_id, node_ids[limit]);
        assert_eq!(GossipAction::Noop, action);
        check_holders(&node_ids[..0], &gossip_table, &data_id);

        // Time the finished data out, then check same partial data causes `GetRemainder` to be
        // returned as per a completely new entry.
        let millis = TimeDiff::from_str(DEFAULT_FINISHED_ENTRY_DURATION)
            .unwrap()
            .millis();
        Instant::advance_time(millis + 1);
        let action = gossip_table.new_partial_data(&data_id, node_ids[0]);
        let expected = GossipAction::GetRemainder {
            holder: node_ids[0],
        };
        assert_eq!(expected, action);
        check_holders(&node_ids[..1], &gossip_table, &data_id);
    }

    #[test]
    fn should_noop_if_we_have_partial_data_and_get_gossip_response() {
        let _ = logging::init();
        let mut rng = crate::new_rng();
        let node_id = NodeId::random(&mut rng);
        let data_id: u64 = rng.gen();

        let mut gossip_table = GossipTable::new(Config::default());

        let _ = gossip_table.new_partial_data(&data_id, node_id);

        let action = gossip_table.we_infected(&data_id, node_id);
        assert_eq!(GossipAction::AwaitingRemainder, action);

        let action = gossip_table.already_infected(&data_id, node_id);
        assert_eq!(GossipAction::AwaitingRemainder, action);
    }

    #[test]
    fn new_complete_data() {
        let _ = logging::init();
        let mut rng = crate::new_rng();
        let node_ids = random_node_ids(&mut rng);
        let data_id: u64 = rng.gen();

        let mut gossip_table = GossipTable::new(Config::default());

        // Check new complete data from us causes `ShouldGossip` to be returned.
        let action = gossip_table.new_complete_data(&data_id, None);
        let expected = GossipAction::ShouldGossip(ShouldGossip {
            count: EXPECTED_DEFAULT_INFECTION_TARGET,
            exclude_peers: HashSet::new(),
            is_already_held: false,
        });
        assert_eq!(expected, action);
        check_holders(&node_ids[..0], &gossip_table, &data_id);

        // Check same complete data from other source causes `Noop` to be returned since we still
        // have all gossip requests in flight.  Check it updates holders.
        let action = gossip_table.new_complete_data(&data_id, Some(node_ids[0]));
        assert_eq!(GossipAction::Noop, action);
        check_holders(&node_ids[..1], &gossip_table, &data_id);

        // Check receiving a gossip response, causes `ShouldGossip` to be returned and holders
        // updated.
        let action = gossip_table.already_infected(&data_id, node_ids[1]);
        let expected = GossipAction::ShouldGossip(ShouldGossip {
            count: 1,
            exclude_peers: node_ids[..2].iter().cloned().collect(),
            is_already_held: true,
        });
        assert_eq!(expected, action);
        check_holders(&node_ids[..2], &gossip_table, &data_id);

        let action = gossip_table.new_complete_data(&data_id, Some(node_ids[2]));
        assert_eq!(GossipAction::Noop, action);
        check_holders(&node_ids[..3], &gossip_table, &data_id);

        // Finish the gossip by reporting enough non-infections, then check same complete data
        // causes `Noop` to be returned and holders cleared.
        let limit = 3 + EXPECTED_DEFAULT_INFECTION_TARGET;
        for node_id in &node_ids[3..limit] {
            let _ = gossip_table.we_infected(&data_id, *node_id);
        }
        let action = gossip_table.new_complete_data(&data_id, None);
        assert_eq!(GossipAction::Noop, action);
        check_holders(&node_ids[..0], &gossip_table, &data_id);

        // Time the finished data out, then check same complete data causes `ShouldGossip` to be
        // returned as per a completely new entry.
        let millis = TimeDiff::from_str(DEFAULT_FINISHED_ENTRY_DURATION)
            .unwrap()
            .millis();
        Instant::advance_time(millis + 1);
        let action = gossip_table.new_complete_data(&data_id, Some(node_ids[0]));
        let expected = GossipAction::ShouldGossip(ShouldGossip {
            count: EXPECTED_DEFAULT_INFECTION_TARGET,
            exclude_peers: node_ids[..1].iter().cloned().collect(),
            is_already_held: false,
        });
        assert_eq!(expected, action);
        check_holders(&node_ids[..1], &gossip_table, &data_id);
    }

    #[test]
    fn should_terminate_via_infection_limit() {
        let _ = logging::init();
        let mut rng = crate::new_rng();
        let node_ids = random_node_ids(&mut rng);
        let data_id: u64 = rng.gen();

        let mut gossip_table = GossipTable::new(Config::default());

        // Add new complete data from us and check two infections doesn't cause us to stop
        // gossiping.
        let _ = gossip_table.new_complete_data(&data_id, None);
        let limit = EXPECTED_DEFAULT_INFECTION_TARGET - 1;
        for node_id in node_ids.iter().take(limit) {
            let action = gossip_table.we_infected(&data_id, *node_id);
            assert_eq!(GossipAction::Noop, action);
            assert!(!gossip_table.finished.contains(&data_id));
        }

        // Check recording an infection from an already-recorded infectee doesn't cause us to stop
        // gossiping.
        let action = gossip_table.we_infected(&data_id, node_ids[limit - 1]);
        let expected = GossipAction::ShouldGossip(ShouldGossip {
            count: 1,
            exclude_peers: node_ids[..limit].iter().cloned().collect(),
            is_already_held: true,
        });
        assert_eq!(expected, action);
        assert!(!gossip_table.finished.contains(&data_id));

        // Check third new infection does cause us to stop gossiping.
        let action = gossip_table.we_infected(&data_id, node_ids[limit]);
        assert_eq!(GossipAction::AnnounceFinished, action);
        assert!(gossip_table.finished.contains(&data_id));
    }

    #[test]
    fn should_terminate_via_incoming_gossip() {
        let _ = logging::init();
        let mut rng = crate::new_rng();
        let node_ids = random_node_ids(&mut rng);
        let data_id1: u64 = rng.gen();
        let data_id2: u64 = rng.gen();

        let mut gossip_table = GossipTable::new(Config::default());

        // Take the two items close to the termination condition of holder count by simulating
        // receiving several incoming gossip requests.  Each should remain unfinished.
        let limit = EXPECTED_DEFAULT_HOLDERS_LIMIT - 1;
        for node_id in node_ids.iter().take(limit) {
            let _ = gossip_table.new_partial_data(&data_id1, *node_id);
            assert!(!gossip_table.finished.contains(&data_id1));

            let _ = gossip_table.new_complete_data(&data_id2, Some(*node_id));
            assert!(!gossip_table.finished.contains(&data_id2));
        }

        // Simulate receiving a final gossip request for each, which should cause them both to be
        // moved to the `finished` collection.
        let action =
            gossip_table.new_partial_data(&data_id1, node_ids[EXPECTED_DEFAULT_HOLDERS_LIMIT]);
        assert!(gossip_table.finished.contains(&data_id1));
        assert_eq!(GossipAction::AnnounceFinished, action);

        let action = gossip_table
            .new_complete_data(&data_id2, Some(node_ids[EXPECTED_DEFAULT_HOLDERS_LIMIT]));
        assert!(gossip_table.finished.contains(&data_id2));
        assert_eq!(GossipAction::AnnounceFinished, action);
    }

    #[test]
    fn should_terminate_via_checking_timeout() {
        let _ = logging::init();
        let mut rng = crate::new_rng();
        let node_ids = random_node_ids(&mut rng);
        let data_id: u64 = rng.gen();

        let mut gossip_table = GossipTable::new(Config::default());

        // Take the item close to the termination condition of holder count by simulating receiving
        // several incoming gossip requests.  It should remain unfinished.
        let limit = EXPECTED_DEFAULT_HOLDERS_LIMIT - 1;
        for node_id in node_ids.iter().take(limit) {
            let _ = gossip_table.new_complete_data(&data_id, Some(*node_id));
            assert!(!gossip_table.finished.contains(&data_id));
        }

        // Simulate a gossip response timing out, which should cause the item to be moved to the
        // `finished` collection.
        let action = gossip_table.check_timeout(&data_id, node_ids[EXPECTED_DEFAULT_HOLDERS_LIMIT]);
        assert!(gossip_table.finished.contains(&data_id));
        assert_eq!(GossipAction::AnnounceFinished, action);
    }

    #[test]
    fn should_terminate_via_reducing_in_flight_count() {
        let _ = logging::init();
        let mut rng = crate::new_rng();
        let data_id: u64 = rng.gen();

        let mut gossip_table = GossipTable::new(Config::default());

        // Take the item close to the termination condition of in-flight count reaching 0.  It
        // should remain unfinished.
        let _ = gossip_table.new_complete_data(&data_id, None);
        let limit = EXPECTED_DEFAULT_INFECTION_TARGET - 1;
        assert!(!gossip_table.reduce_in_flight_count(&data_id, limit));
        assert!(!gossip_table.finished.contains(&data_id));

        // Reduce the in-flight count to 0, which should cause the item to be moved to the
        // `finished` collection.
        assert!(gossip_table.reduce_in_flight_count(&data_id, 1));
        assert!(gossip_table.finished.contains(&data_id));

        // Check that calling this again has no effect and continues to return `false`.
        assert!(!gossip_table.reduce_in_flight_count(&data_id, 1));
        assert!(gossip_table.finished.contains(&data_id));
    }

    #[test]
    fn should_terminate_via_saturation() {
        let _ = logging::init();
        let mut rng = crate::new_rng();
        let node_ids = random_node_ids(&mut rng);
        let data_id: u64 = rng.gen();

        let mut gossip_table = GossipTable::new(Config::default());

        // Add new complete data with 14 non-infections and check this doesn't cause us to stop
        // gossiping.
        let _ = gossip_table.new_complete_data(&data_id, None);
        let limit = EXPECTED_DEFAULT_HOLDERS_LIMIT - 1;
        for (index, node_id) in node_ids.iter().enumerate().take(limit) {
            let action = gossip_table.already_infected(&data_id, *node_id);
            let expected = GossipAction::ShouldGossip(ShouldGossip {
                count: 1,
                exclude_peers: node_ids[..(index + 1)].iter().cloned().collect(),
                is_already_held: true,
            });
            assert_eq!(expected, action);
        }

        // Check recording a non-infection from an already-recorded holder doesn't cause us to stop
        // gossiping.
        let action = gossip_table.already_infected(&data_id, node_ids[0]);
        let expected = GossipAction::ShouldGossip(ShouldGossip {
            count: 1,
            exclude_peers: node_ids[..limit].iter().cloned().collect(),
            is_already_held: true,
        });
        assert_eq!(expected, action);

        // Check 15th non-infection does cause us to stop gossiping.
        let action = gossip_table.we_infected(&data_id, node_ids[limit]);
        assert_eq!(GossipAction::AnnounceFinished, action);
    }

    #[test]
    fn should_not_terminate_below_infection_limit_and_saturation() {
        let _ = logging::init();
        let mut rng = crate::new_rng();
        let node_ids = random_node_ids(&mut rng);
        let data_id: u64 = rng.gen();

        let mut gossip_table = GossipTable::new(Config::default());

        // Add new complete data with 2 infections and 11 non-infections.
        let _ = gossip_table.new_complete_data(&data_id, None);
        let infection_limit = EXPECTED_DEFAULT_INFECTION_TARGET - 1;
        for node_id in &node_ids[0..infection_limit] {
            let _ = gossip_table.we_infected(&data_id, *node_id);
        }

        let holders_limit = EXPECTED_DEFAULT_HOLDERS_LIMIT - 2;
        for node_id in &node_ids[infection_limit..holders_limit] {
            let _ = gossip_table.already_infected(&data_id, *node_id);
        }

        // Check adding 12th non-infection doesn't cause us to stop gossiping.
        let action = gossip_table.already_infected(&data_id, node_ids[holders_limit]);
        let expected = GossipAction::ShouldGossip(ShouldGossip {
            count: 1,
            exclude_peers: node_ids[..(holders_limit + 1)].iter().cloned().collect(),
            is_already_held: true,
        });
        assert_eq!(expected, action);
    }

    #[test]
    fn check_timeout_should_detect_holder() {
        let _ = logging::init();
        let mut rng = crate::new_rng();
        let node_ids = random_node_ids(&mut rng);
        let data_id: u64 = rng.gen();

        let mut gossip_table = GossipTable::new(Config::default());

        // Add new complete data and get a response from node 0 only.
        let _ = gossip_table.new_complete_data(&data_id, None);
        let _ = gossip_table.we_infected(&data_id, node_ids[0]);

        // check_timeout for node 0 should return Noop, and for node 1 it should represent a timed
        // out response and return ShouldGossip.
        let action = gossip_table.check_timeout(&data_id, node_ids[0]);
        assert_eq!(GossipAction::Noop, action);

        let action = gossip_table.check_timeout(&data_id, node_ids[1]);
        let expected = GossipAction::ShouldGossip(ShouldGossip {
            count: 1,
            exclude_peers: node_ids[..=1].iter().cloned().collect(),
            is_already_held: true,
        });
        assert_eq!(expected, action);
    }

    #[test]
    #[cfg_attr(
        debug_assertions,
        should_panic(expected = "shouldn't check timeout for a gossip response for partial data")
    )]
    fn check_timeout_should_panic_for_partial_copy() {
        let _ = logging::init();
        let mut rng = crate::new_rng();
        let node_ids = random_node_ids(&mut rng);
        let data_id: u64 = rng.gen();

        let mut gossip_table = GossipTable::new(Config::default());
        let _ = gossip_table.new_partial_data(&data_id, node_ids[0]);
        let _ = gossip_table.check_timeout(&data_id, node_ids[0]);
    }

    #[test]
    fn should_remove_holder_if_unresponsive() {
        let _ = logging::init();
        let mut rng = crate::new_rng();
        let node_ids = random_node_ids(&mut rng);
        let data_id: u64 = rng.gen();

        let mut gossip_table = GossipTable::new(Config::default());

        // Add new partial data from nodes 0 and 1.
        let _ = gossip_table.new_partial_data(&data_id, node_ids[0]);
        let _ = gossip_table.new_partial_data(&data_id, node_ids[1]);

        // Node 0 should be removed from the holders since it hasn't provided us with the full data,
        // and we should be told to get the remainder from node 1.
        let action = gossip_table.remove_holder_if_unresponsive(&data_id, node_ids[0]);
        let expected = GossipAction::GetRemainder {
            holder: node_ids[1],
        };
        assert_eq!(expected, action);
        check_holders(&node_ids[1..2], &gossip_table, &data_id);

        // Node 1 should be removed from the holders since it hasn't provided us with the full data,
        // and the entry should be removed since there are no more holders.
        let action = gossip_table.remove_holder_if_unresponsive(&data_id, node_ids[1]);
        assert_eq!(GossipAction::Noop, action);
        assert!(!gossip_table.current.contains_key(&data_id));
        assert!(!gossip_table.finished.contains(&data_id));
    }

    #[test]
    fn should_not_remove_holder_if_responsive() {
        let _ = logging::init();
        let mut rng = crate::new_rng();
        let node_ids = random_node_ids(&mut rng);
        let data_id: u64 = rng.gen();

        let mut gossip_table = GossipTable::new(Config::default());

        // Add new partial data from node 0 and record that we have received the full data from it.
        let _ = gossip_table.new_partial_data(&data_id, node_ids[0]);
        let _ = gossip_table.new_complete_data(&data_id, Some(node_ids[0]));

        // Node 0 should remain as a holder since we now hold the complete data.
        let action = gossip_table.remove_holder_if_unresponsive(&data_id, node_ids[0]);
        assert_eq!(GossipAction::Noop, action); // Noop as all RPCs are still in-flight
        check_holders(&node_ids[..1], &gossip_table, &data_id);
        assert!(gossip_table.current.contains_key(&data_id));
    }

    #[test]
    fn should_force_finish() {
        let _ = logging::init();
        let mut rng = crate::new_rng();
        let node_ids = random_node_ids(&mut rng);
        let data_id: u64 = rng.gen();

        let mut gossip_table = GossipTable::new(Config::default());

        // Add new partial data from node 0, then forcibly finish gossiping.
        let _ = gossip_table.new_partial_data(&data_id, node_ids[0]);
        assert!(gossip_table.force_finish(&data_id));
        assert!(gossip_table.finished.contains(&data_id));

        // Ensure forcibly finishing the same data returns `false`.
        assert!(!gossip_table.force_finish(&data_id));
    }

    #[test]
    fn should_purge() {
        let _ = logging::init();
        let mut rng = crate::new_rng();
        let node_ids = random_node_ids(&mut rng);
        let data_id: u64 = rng.gen();

        let mut gossip_table = GossipTable::new(Config::default());

        // Add new complete data and finish via infection limit.
        let _ = gossip_table.new_complete_data(&data_id, None);
        for node_id in &node_ids[0..EXPECTED_DEFAULT_INFECTION_TARGET] {
            let _ = gossip_table.we_infected(&data_id, *node_id);
        }
        assert!(gossip_table.finished.contains(&data_id));

        // Time the finished data out and check it has been purged.
        let millis = TimeDiff::from_str(DEFAULT_FINISHED_ENTRY_DURATION)
            .unwrap()
            .millis();
        Instant::advance_time(millis + 1);
        gossip_table.purge_finished();
        assert!(!gossip_table.finished.contains(&data_id));

        // Add new complete data and forcibly finish.
        let _ = gossip_table.new_complete_data(&data_id, None);
        assert!(gossip_table.force_finish(&data_id));
        assert!(gossip_table.finished.contains(&data_id));

        // Time the finished data out and check it has been purged.
        Instant::advance_time(millis + 1);
        gossip_table.purge_finished();
        assert!(!gossip_table.finished.contains(&data_id));
    }

    #[test]
    fn timeouts_purge_in_order() {
        let mut timeouts = Timeouts::new();
        let now = Instant::now();
        let later_100 = now + Duration::from_millis(100);
        let later_200 = now + Duration::from_millis(200);

        // Timeouts are added and purged in chronological order.
        timeouts.push(now, 0);
        timeouts.push(later_100, 1);
        timeouts.push(later_200, 2);

        let now_after_time_travel = now + Duration::from_millis(10);
        let purged = timeouts.purge(&now_after_time_travel).collect::<Vec<i32>>();

        assert_eq!(purged, vec![0]);
    }

    #[test]
    fn timeouts_depends_on_binary_search_by_implementation() {
        // This test is meant to document the dependency of
        // Timeouts::purge on https://doc.rust-lang.org/std/vec/struct.Vec.html#method.binary_search_by.
        // If this test is failing then it's reasonable to believe that the implementation of
        // binary_search_by has been updated.
        let mut timeouts = Timeouts::new();
        let now = Instant::now();
        let later_100 = now + Duration::from_millis(100);
        let later_200 = now + Duration::from_millis(200);
        let later_300 = now + Duration::from_millis(300);
        let later_400 = now + Duration::from_millis(400);
        let later_500 = now + Duration::from_millis(500);
        let later_600 = now + Duration::from_millis(600);

        timeouts.push(later_100, 1);
        timeouts.push(later_200, 2);
        timeouts.push(later_300, 3);

        // If a node's system time was changed to a time earlier than
        // the earliest timeout, and a new timeout is added with an instant
        // corresponding to this new early time, then this would make the earliest
        // timeout the LAST timeout in the vec.
        // [100 < 200 < 300 > 0]
        timeouts.push(now, 0);

        let now_after_time_travel = now + Duration::from_millis(10);
        // Intuitively, we would expect [1,2,3,0] to be in the "purged" vec here.
        // This is not the case because we're using binary_search_by, which (currently)
        // is implemented with logic that checks if a, b, ... z are in a consistent order.
        // in this case, the order that we've established is a < b < ... < z for each element in the
        // vec, but we broke that order by inserting '0' last, and for some reason,
        // binary_search_by won't find this unless there is a number > n occurring AFTER n
        // in the vec.

        let purged = timeouts.purge(&now_after_time_travel).collect::<Vec<i32>>();
        let empty: Vec<i32> = vec![];

        // This isn't a problem and the order will eventually
        // be restored.
        assert_eq!(purged, empty);

        timeouts.push(later_400, 4);
        timeouts.push(later_500, 5);
        timeouts.push(later_600, 6);

        // Now, we advance time another 10 ms and purge again.
        // In this scenario, timeouts with a later time are added after our
        // improperly ordered "now" timeout
        // [100 < 200 < 300 > 0 < 400 < 500 < 600]
        let now_after_time_travel = now + Duration::from_millis(20);
        let purged = timeouts.purge(&now_after_time_travel).collect::<Vec<i32>>();
        let expected = [1, 2, 3, 0];

        assert_eq!(purged, expected);

        // After the previous purge, an order is restored where a < b for consecutive elements in
        // the vec. [400 < 500 < 600], so, purging timeouts up to 610 will properly clear
        // the vec.
        let now_after_time_travel = now + Duration::from_millis(610);
        let purged = timeouts.purge(&now_after_time_travel).collect::<Vec<i32>>();
        let expected = [4, 5, 6];

        assert_eq!(purged, expected);
        assert_eq!(0, timeouts.values.len());
    }
}
