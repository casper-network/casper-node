#[cfg(not(test))]
use std::time::Instant;
use std::{
    collections::{hash_map::Entry, HashMap, HashSet},
    fmt::Display,
    hash::Hash,
    time::Duration,
};

use datasize::DataSize;
#[cfg(test)]
use fake_instant::FakeClock as Instant;
use tracing::warn;

use super::Config;
#[cfg(test)]
use super::Error;
use crate::types::NodeId;

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
            let holder = self
                .holders
                .iter()
                .next()
                .expect("holders cannot be empty if we don't hold the data")
                .clone();
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
    finished_timeouts: Timeouts<T>,
    /// Data IDs for which gossiping has been paused (likely due to detecting that the data was not
    /// correct as per our current knowledge).  Such data could later be decided as still requiring
    /// to be gossiped, so we retain the `State` part here in order to resume gossiping.
    paused: HashMap<T, State>,
    /// Timeouts for removal of items from the `paused` cache.
    paused_timeouts: Timeouts<T>,
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

    /// Number of items for which gossipping is currently paused.
    pub fn items_paused(&self) -> usize {
        self.paused.len()
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
            finished_timeouts: Timeouts::new(),
            paused: HashMap::new(),
            paused_timeouts: Timeouts::new(),
            infection_target: usize::from(config.infection_target()),
            holders_limit,
            finished_entry_duration: Duration::from_secs(config.finished_entry_duration_secs()),
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
            return GossipAction::Noop;
        }

        if let Some(state) = self.paused.get_mut(data_id) {
            let _ = state.holders.insert(holder);
            return GossipAction::Noop;
        }

        match self.current.entry(*data_id) {
            Entry::Occupied(mut entry) => {
                let is_new = false;
                let state = entry.get_mut();
                let _ = state.holders.insert(holder);
                state.action(self.infection_target, self.holders_limit, is_new)
            }
            Entry::Vacant(entry) => {
                let is_new = true;
                let state = entry.insert(State::default());
                let _ = state.holders.insert(holder);
                state.action(self.infection_target, self.holders_limit, is_new)
            }
        }
    }

    /// We received or generated potentially new data with given ID.  If received from a peer,
    /// its ID should be passed in `maybe_holder`.  If received from a client or generated on this
    /// node, `maybe_holder` should be `None`.
    ///
    /// This should only be called once we hold everything locally we need to be able to gossip it
    /// onwards.  If we aren't able to gossip this data yet, call `new_data_id` instead.
    ///
    /// Returns whether we should gossip it, and a list of peers to exclude.
    pub(crate) fn new_complete_data(
        &mut self,
        data_id: &T,
        maybe_holder: Option<NodeId>,
    ) -> Option<ShouldGossip> {
        self.purge_finished();

        if self.finished.contains(data_id) {
            return None;
        }

        let update = |state: &mut State| {
            state.holders.extend(maybe_holder);
            state.held_by_us = true;
        };

        if let Some(state) = self.paused.get_mut(data_id) {
            update(state);
            return None;
        }

        let action = match self.current.entry(*data_id) {
            Entry::Occupied(mut entry) => {
                let state = entry.get_mut();
                update(state);
                let is_new = false;
                state.action(self.infection_target, self.holders_limit, is_new)
            }
            Entry::Vacant(entry) => {
                let state = entry.insert(State::default());
                update(state);
                let is_new = true;
                state.action(self.infection_target, self.holders_limit, is_new)
            }
        };

        match action {
            GossipAction::ShouldGossip(should_gossip) => Some(should_gossip),
            GossipAction::Noop => None,
            GossipAction::GetRemainder { .. } | GossipAction::AwaitingRemainder => {
                unreachable!("can't be waiting for remainder since we hold the complete data")
            }
        }
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
        let infection_target = self.infection_target;
        let holders_limit = self.holders_limit;
        let update = |state: &mut State| {
            if !state.held_by_us {
                warn!(
                    %data_id,
                    %peer, "shouldn't have received a gossip response for partial data"
                );
                return None;
            }
            let _ = state.holders.insert(peer.clone());
            if by_us {
                let _ = state.infected_by_us.insert(peer.clone());
            }
            state.in_flight_count = state.in_flight_count.saturating_sub(1);
            Some(state.is_finished(infection_target, holders_limit))
        };

        let is_finished = if let Some(state) = self.current.get_mut(data_id) {
            let is_finished = match update(state) {
                Some(is_finished) => is_finished,
                None => return GossipAction::Noop,
            };
            if !is_finished {
                let is_new = false;
                return state.action(self.infection_target, self.holders_limit, is_new);
            }
            true
        } else {
            false
        };

        if is_finished {
            let _ = self.current.remove(data_id);
            let timeout = Instant::now() + self.finished_entry_duration;
            let _ = self.finished.insert(*data_id);
            let _ = self.finished_timeouts.push(timeout, *data_id);
            return GossipAction::Noop;
        }

        let is_finished = if let Some(state) = self.paused.get_mut(data_id) {
            match update(state) {
                Some(is_finished) => is_finished,
                None => return GossipAction::Noop,
            }
        } else {
            false
        };

        if is_finished {
            let _ = self.paused.remove(data_id);
            let timeout = Instant::now() + self.finished_entry_duration;
            let _ = self.finished.insert(*data_id);
            let _ = self.finished_timeouts.push(timeout, *data_id);
        }

        GossipAction::Noop
    }

    /// Directly reduces the in-flight count of gossip requests for the given item by the given
    /// amount.
    ///
    /// This should be called if, after trying to gossip to a given number of peers, we find that
    /// we've not been able to select enough peers.  Without this reduction, the given gossip item
    /// would never move from `current` to `finished` or `paused`, and hence would never be purged.
    pub(crate) fn reduce_in_flight_count(&mut self, data_id: &T, reduce_by: usize) {
        if let Some(state) = self.current.get_mut(data_id) {
            state.in_flight_count = state.in_flight_count.saturating_sub(reduce_by);
        }
    }

    /// Checks if gossip request we sent timed out.
    ///
    /// If the peer is already counted as a holder, it has previously responded and this method
    /// returns Noop.  Otherwise it has timed out and we return the appropriate action to take.
    pub(crate) fn check_timeout(&mut self, data_id: &T, peer: NodeId) -> GossipAction {
        if let Some(state) = self.current.get_mut(data_id) {
            debug_assert!(
                state.held_by_us,
                "shouldn't check timeout for a gossip response for partial data"
            );

            if !state.holders.contains(&peer) {
                // Add the peer as a holder just to avoid retrying it.
                let _ = state.holders.insert(peer);
                state.in_flight_count = state.in_flight_count.saturating_sub(1);
                let is_new = false;
                return state.action(self.infection_target, self.holders_limit, is_new);
            }
        }

        GossipAction::Noop
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
                if state.holders.is_empty() {
                    // We don't hold the full data, and we don't know any holders - pause the entry
                    return GossipAction::Noop;
                }
            }
            let is_new = !state.held_by_us;
            let action = state.action(self.infection_target, self.holders_limit, is_new);
            let _ = self.current.insert(*data_id, state);
            return action;
        }

        if let Some(state) = self.paused.get_mut(data_id) {
            if !state.held_by_us {
                let _ = state.holders.remove(&peer);
            }
        }

        GossipAction::Noop
    }

    /// We have deemed the data not suitable for gossiping further.  If left in paused state, the
    /// entry will eventually be purged, as for finished entries.
    pub(crate) fn pause(&mut self, data_id: &T) {
        if let Some(mut state) = self.current.remove(data_id) {
            state.in_flight_count = 0;
            let timeout = Instant::now() + self.finished_entry_duration;
            let _ = self.paused.insert(*data_id, state);
            let _ = self.paused_timeouts.push(timeout, *data_id);
        }
    }

    /// Resumes gossiping of paused entry.
    ///
    /// Returns an error if gossiping this data is not in a paused state.
    // TODO - remove lint relaxation once the method is used.
    #[cfg(test)]
    pub(crate) fn resume(&mut self, data_id: &T) -> Result<GossipAction, Error> {
        let mut state = self.paused.remove(data_id).ok_or(Error::NotPaused)?;
        let is_new = !state.held_by_us;
        let action = state.action(self.infection_target, self.holders_limit, is_new);
        let _ = self.current.insert(*data_id, state);
        Ok(action)
    }

    /// Retains only those finished entries which still haven't timed out.
    fn purge_finished(&mut self) {
        let now = Instant::now();

        for expired_finished in self.finished_timeouts.purge(&now) {
            let _ = self.finished.remove(&expired_finished);
        }

        for expired_paused in self.paused_timeouts.purge(&now) {
            let _ = self.paused.remove(&expired_paused);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, iter};

    use rand::Rng;
    use test::Bencher;

    use super::{super::config::DEFAULT_FINISHED_ENTRY_DURATION_SECS, *};
    use crate::{crypto::hash::Digest, testing::TestRng, types::DeployHash, utils::DisplayIter};

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
            .or_else(|| gossip_table.paused.get(data_id))
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
        let action = gossip_table.new_partial_data(&data_id, node_ids[0].clone());
        let expected = GossipAction::GetRemainder {
            holder: node_ids[0].clone(),
        };
        assert_eq!(expected, action);
        check_holders(&node_ids[..1], &gossip_table, &data_id);

        // Check same partial data from same source causes `AwaitingRemainder` to be returned.
        let action = gossip_table.new_partial_data(&data_id, node_ids[0].clone());
        assert_eq!(GossipAction::AwaitingRemainder, action);
        check_holders(&node_ids[..1], &gossip_table, &data_id);

        // Check same partial data from different source causes `AwaitingRemainder` to be returned
        // and holders updated.
        let action = gossip_table.new_partial_data(&data_id, node_ids[1].clone());
        assert_eq!(GossipAction::AwaitingRemainder, action);
        check_holders(&node_ids[..2], &gossip_table, &data_id);

        // Pause gossiping and check same partial data from third source causes `Noop` to be
        // returned and holders updated.
        gossip_table.pause(&data_id);
        let action = gossip_table.new_partial_data(&data_id, node_ids[2].clone());
        assert_eq!(GossipAction::Noop, action);
        check_holders(&node_ids[..3], &gossip_table, &data_id);

        // Reset the data and check same partial data from fourth source causes `AwaitingRemainder`
        // to be returned and holders updated.
        gossip_table.resume(&data_id).unwrap();
        let action = gossip_table.new_partial_data(&data_id, node_ids[3].clone());
        assert_eq!(GossipAction::AwaitingRemainder, action);
        check_holders(&node_ids[..4], &gossip_table, &data_id);

        // Finish the gossip by reporting three infections, then check same partial data causes
        // `Noop` to be returned and holders cleared.
        let _ = gossip_table.new_complete_data(&data_id, Some(node_ids[0].clone()));
        let limit = 4 + EXPECTED_DEFAULT_INFECTION_TARGET;
        for node_id in &node_ids[4..limit] {
            let _ = gossip_table.we_infected(&data_id, node_id.clone());
        }
        let action = gossip_table.new_partial_data(&data_id, node_ids[limit].clone());
        assert_eq!(GossipAction::Noop, action);
        check_holders(&node_ids[..0], &gossip_table, &data_id);

        // Time the finished data out, then check same partial data causes `GetRemainder` to be
        // returned as per a completely new entry.
        Instant::advance_time(DEFAULT_FINISHED_ENTRY_DURATION_SECS * 1_000 + 1);
        let action = gossip_table.new_partial_data(&data_id, node_ids[0].clone());
        let expected = GossipAction::GetRemainder {
            holder: node_ids[0].clone(),
        };
        assert_eq!(expected, action);
        check_holders(&node_ids[..1], &gossip_table, &data_id);
    }

    #[test]
    fn should_noop_if_we_have_partial_data_and_get_gossip_response() {
        let mut rng = crate::new_rng();
        let node_id = NodeId::random(&mut rng);
        let data_id: u64 = rng.gen();

        let mut gossip_table = GossipTable::new(Config::default());

        let _ = gossip_table.new_partial_data(&data_id, node_id.clone());

        let action = gossip_table.we_infected(&data_id, node_id.clone());
        assert_eq!(GossipAction::Noop, action);

        let action = gossip_table.already_infected(&data_id, node_id);
        assert_eq!(GossipAction::Noop, action);
    }

    #[test]
    fn new_complete_data() {
        let mut rng = crate::new_rng();
        let node_ids = random_node_ids(&mut rng);
        let data_id: u64 = rng.gen();

        let mut gossip_table = GossipTable::new(Config::default());

        // Check new complete data from us causes `ShouldGossip` to be returned.
        let action = gossip_table.new_complete_data(&data_id, None);
        let expected = Some(ShouldGossip {
            count: EXPECTED_DEFAULT_INFECTION_TARGET,
            exclude_peers: HashSet::new(),
            is_already_held: false,
        });
        assert_eq!(expected, action);
        check_holders(&node_ids[..0], &gossip_table, &data_id);

        // Check same complete data from other source causes `Noop` to be returned since we still
        // have all gossip requests in flight.  Check it updates holders.
        let action = gossip_table.new_complete_data(&data_id, Some(node_ids[0].clone()));
        assert!(action.is_none());
        check_holders(&node_ids[..1], &gossip_table, &data_id);

        // Check receiving a gossip response, causes `ShouldGossip` to be returned and holders
        // updated.
        let action = gossip_table.already_infected(&data_id, node_ids[1].clone());
        let expected = GossipAction::ShouldGossip(ShouldGossip {
            count: 1,
            exclude_peers: node_ids[..2].iter().cloned().collect(),
            is_already_held: true,
        });
        assert_eq!(expected, action);
        check_holders(&node_ids[..2], &gossip_table, &data_id);

        // Pause gossiping and check same complete data from third source causes `Noop` to be
        // returned and holders updated.
        gossip_table.pause(&data_id);
        let action = gossip_table.new_complete_data(&data_id, Some(node_ids[2].clone()));
        assert!(action.is_none());
        check_holders(&node_ids[..3], &gossip_table, &data_id);

        // Reset the data and check same complete data from fourth source causes Noop` to be
        // returned since we still have all gossip requests in flight.  Check it updates holders.
        let action = gossip_table.resume(&data_id).unwrap();
        let expected = GossipAction::ShouldGossip(ShouldGossip {
            count: EXPECTED_DEFAULT_INFECTION_TARGET,
            exclude_peers: node_ids[..3].iter().cloned().collect(),
            is_already_held: true,
        });
        assert_eq!(expected, action);

        let action = gossip_table.new_complete_data(&data_id, Some(node_ids[3].clone()));
        assert!(action.is_none());
        check_holders(&node_ids[..4], &gossip_table, &data_id);

        // Finish the gossip by reporting enough non-infections, then check same complete data
        // causes `Noop` to be returned and holders cleared.
        let limit = 4 + EXPECTED_DEFAULT_INFECTION_TARGET;
        for node_id in &node_ids[4..limit] {
            let _ = gossip_table.we_infected(&data_id, node_id.clone());
        }
        let action = gossip_table.new_complete_data(&data_id, None);
        assert!(action.is_none());
        check_holders(&node_ids[..0], &gossip_table, &data_id);

        // Time the finished data out, then check same complete data causes `ShouldGossip` to be
        // returned as per a completely new entry.
        Instant::advance_time(DEFAULT_FINISHED_ENTRY_DURATION_SECS * 1_000 + 1);
        let action = gossip_table.new_complete_data(&data_id, Some(node_ids[0].clone()));
        let expected = Some(ShouldGossip {
            count: EXPECTED_DEFAULT_INFECTION_TARGET,
            exclude_peers: node_ids[..1].iter().cloned().collect(),
            is_already_held: false,
        });
        assert_eq!(expected, action);
        check_holders(&node_ids[..1], &gossip_table, &data_id);
    }

    #[test]
    fn should_terminate_via_infection_limit() {
        let mut rng = crate::new_rng();
        let node_ids = random_node_ids(&mut rng);
        let data_id: u64 = rng.gen();

        let mut gossip_table = GossipTable::new(Config::default());

        // Add new complete data from us and check two infections doesn't cause us to stop
        // gossiping.
        let _ = gossip_table.new_complete_data(&data_id, None);
        let limit = EXPECTED_DEFAULT_INFECTION_TARGET - 1;
        for node_id in node_ids.iter().take(limit) {
            let action = gossip_table.we_infected(&data_id, node_id.clone());
            assert_eq!(GossipAction::Noop, action);
            assert!(!gossip_table.finished.contains(&data_id));
        }

        // Check recording an infection from an already-recorded infectee doesn't cause us to stop
        // gossiping.
        let action = gossip_table.we_infected(&data_id, node_ids[limit - 1].clone());
        let expected = GossipAction::ShouldGossip(ShouldGossip {
            count: 1,
            exclude_peers: node_ids[..limit].iter().cloned().collect(),
            is_already_held: true,
        });
        assert_eq!(expected, action);
        assert!(!gossip_table.finished.contains(&data_id));

        // Check third new infection does cause us to stop gossiping.
        let action = gossip_table.we_infected(&data_id, node_ids[limit].clone());
        assert_eq!(GossipAction::Noop, action);
        assert!(gossip_table.finished.contains(&data_id));
    }

    #[test]
    fn should_terminate_via_saturation() {
        let mut rng = crate::new_rng();
        let node_ids = random_node_ids(&mut rng);
        let data_id: u64 = rng.gen();

        let mut gossip_table = GossipTable::new(Config::default());

        // Add new complete data with 14 non-infections and check this doesn't cause us to stop
        // gossiping.
        let _ = gossip_table.new_complete_data(&data_id, None);
        let limit = EXPECTED_DEFAULT_HOLDERS_LIMIT - 1;
        for (index, node_id) in node_ids.iter().enumerate().take(limit) {
            let action = gossip_table.already_infected(&data_id, node_id.clone());
            let expected = GossipAction::ShouldGossip(ShouldGossip {
                count: 1,
                exclude_peers: node_ids[..(index + 1)].iter().cloned().collect(),
                is_already_held: true,
            });
            assert_eq!(expected, action);
        }

        // Check recording a non-infection from an already-recorded holder doesn't cause us to stop
        // gossiping.
        let action = gossip_table.already_infected(&data_id, node_ids[0].clone());
        let expected = GossipAction::ShouldGossip(ShouldGossip {
            count: 1,
            exclude_peers: node_ids[..limit].iter().cloned().collect(),
            is_already_held: true,
        });
        assert_eq!(expected, action);

        // Check 15th non-infection does cause us to stop gossiping.
        let action = gossip_table.we_infected(&data_id, node_ids[limit].clone());
        assert_eq!(GossipAction::Noop, action);
    }

    #[test]
    fn should_not_terminate_below_infection_limit_and_saturation() {
        let mut rng = crate::new_rng();
        let node_ids = random_node_ids(&mut rng);
        let data_id: u64 = rng.gen();

        let mut gossip_table = GossipTable::new(Config::default());

        // Add new complete data with 2 infections and 11 non-infections.
        let _ = gossip_table.new_complete_data(&data_id, None);
        let infection_limit = EXPECTED_DEFAULT_INFECTION_TARGET - 1;
        for node_id in &node_ids[0..infection_limit] {
            let _ = gossip_table.we_infected(&data_id, node_id.clone());
        }

        let holders_limit = EXPECTED_DEFAULT_HOLDERS_LIMIT - 2;
        for node_id in &node_ids[infection_limit..holders_limit] {
            let _ = gossip_table.already_infected(&data_id, node_id.clone());
        }

        // Check adding 12th non-infection doesn't cause us to stop gossiping.
        let action = gossip_table.already_infected(&data_id, node_ids[holders_limit].clone());
        let expected = GossipAction::ShouldGossip(ShouldGossip {
            count: 1,
            exclude_peers: node_ids[..(holders_limit + 1)].iter().cloned().collect(),
            is_already_held: true,
        });
        assert_eq!(expected, action);
    }

    #[test]
    fn check_timeout_should_detect_holder() {
        let mut rng = crate::new_rng();
        let node_ids = random_node_ids(&mut rng);
        let data_id: u64 = rng.gen();

        let mut gossip_table = GossipTable::new(Config::default());

        // Add new complete data and get a response from node 0 only.
        let _ = gossip_table.new_complete_data(&data_id, None);
        let _ = gossip_table.we_infected(&data_id, node_ids[0].clone());

        // check_timeout for node 0 should return Noop, and for node 1 it should represent a timed
        // out response and return ShouldGossip.
        let action = gossip_table.check_timeout(&data_id, node_ids[0].clone());
        assert_eq!(GossipAction::Noop, action);

        let action = gossip_table.check_timeout(&data_id, node_ids[1].clone());
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
        let mut rng = crate::new_rng();
        let node_ids = random_node_ids(&mut rng);
        let data_id: u64 = rng.gen();

        let mut gossip_table = GossipTable::new(Config::default());
        let _ = gossip_table.new_partial_data(&data_id, node_ids[0].clone());
        let _ = gossip_table.check_timeout(&data_id, node_ids[0].clone());
    }

    #[test]
    fn should_remove_holder_if_unresponsive() {
        let mut rng = crate::new_rng();
        let node_ids = random_node_ids(&mut rng);
        let data_id: u64 = rng.gen();

        let mut gossip_table = GossipTable::new(Config::default());

        // Add new partial data from nodes 0 and 1.
        let _ = gossip_table.new_partial_data(&data_id, node_ids[0].clone());
        let _ = gossip_table.new_partial_data(&data_id, node_ids[1].clone());

        // Node 0 should be removed from the holders since it hasn't provided us with the full data,
        // and we should be told to get the remainder from node 1.
        let action = gossip_table.remove_holder_if_unresponsive(&data_id, node_ids[0].clone());
        let expected = GossipAction::GetRemainder {
            holder: node_ids[1].clone(),
        };
        assert_eq!(expected, action);
        check_holders(&node_ids[1..2], &gossip_table, &data_id);

        // Node 1 should be removed from the holders since it hasn't provided us with the full data,
        // and the entry should be removed since there are no more holders.
        let action = gossip_table.remove_holder_if_unresponsive(&data_id, node_ids[1].clone());
        assert_eq!(GossipAction::Noop, action);
        check_holders(&node_ids[..0], &gossip_table, &data_id);
        assert!(!gossip_table.current.contains_key(&data_id));
        assert!(!gossip_table.paused.contains_key(&data_id));

        // Add new partial data from node 2 and check gossiping has been resumed.
        let action = gossip_table.new_partial_data(&data_id, node_ids[2].clone());
        let expected = GossipAction::GetRemainder {
            holder: node_ids[2].clone(),
        };
        assert_eq!(expected, action);
        check_holders(&node_ids[2..3], &gossip_table, &data_id);

        // Node 2 should be removed from the holders since it hasn't provided us with the full data,
        // and the entry should be paused since there are no more holders.
        let action = gossip_table.remove_holder_if_unresponsive(&data_id, node_ids[2].clone());
        assert_eq!(GossipAction::Noop, action);
        check_holders(&node_ids[..0], &gossip_table, &data_id);
        assert!(!gossip_table.current.contains_key(&data_id));
        assert!(!gossip_table.paused.contains_key(&data_id));

        // Add new complete data from node 3 and check gossiping has been resumed.
        let action = gossip_table.new_complete_data(&data_id, Some(node_ids[3].clone()));
        let expected = Some(ShouldGossip {
            count: EXPECTED_DEFAULT_INFECTION_TARGET,
            exclude_peers: iter::once(node_ids[3].clone()).collect(),
            is_already_held: false,
        });
        assert_eq!(expected, action);
        check_holders(&node_ids[3..4], &gossip_table, &data_id);
    }

    #[test]
    fn should_not_remove_holder_if_responsive() {
        let mut rng = crate::new_rng();
        let node_ids = random_node_ids(&mut rng);
        let data_id: u64 = rng.gen();

        let mut gossip_table = GossipTable::new(Config::default());

        // Add new partial data from node 0 and record that we have received the full data from it.
        let _ = gossip_table.new_partial_data(&data_id, node_ids[0].clone());
        let _ = gossip_table.new_complete_data(&data_id, Some(node_ids[0].clone()));

        // Node 0 should remain as a holder since we now hold the complete data.
        let action = gossip_table.remove_holder_if_unresponsive(&data_id, node_ids[0].clone());
        assert_eq!(GossipAction::Noop, action); // Noop as all RPCs are still in-flight
        check_holders(&node_ids[..1], &gossip_table, &data_id);
        assert!(gossip_table.current.contains_key(&data_id));
        assert!(!gossip_table.paused.contains_key(&data_id));
    }

    #[test]
    fn should_not_auto_resume_manually_paused() {
        let mut rng = crate::new_rng();
        let node_ids = random_node_ids(&mut rng);
        let data_id: u64 = rng.gen();

        let mut gossip_table = GossipTable::new(Config::default());

        // Add new partial data from node 0, manually pause gossiping, then record that node 0
        // failed to provide the full data.
        let _ = gossip_table.new_partial_data(&data_id, node_ids[0].clone());
        gossip_table.pause(&data_id);
        let action = gossip_table.remove_holder_if_unresponsive(&data_id, node_ids[0].clone());
        assert_eq!(GossipAction::Noop, action);
        check_holders(&node_ids[..0], &gossip_table, &data_id);

        // Add new partial data from node 1 and check gossiping has not been resumed.
        let action = gossip_table.new_partial_data(&data_id, node_ids[1].clone());
        assert_eq!(GossipAction::Noop, action);
        check_holders(&node_ids[1..2], &gossip_table, &data_id);
        assert!(!gossip_table.current.contains_key(&data_id));
        assert!(gossip_table.paused.contains_key(&data_id));
    }

    #[test]
    fn should_purge() {
        let mut rng = crate::new_rng();
        let node_ids = random_node_ids(&mut rng);
        let data_id: u64 = rng.gen();

        let mut gossip_table = GossipTable::new(Config::default());

        // Add new complete data and finish via infection limit.
        let _ = gossip_table.new_complete_data(&data_id, None);
        for node_id in &node_ids[0..EXPECTED_DEFAULT_INFECTION_TARGET] {
            let _ = gossip_table.we_infected(&data_id, node_id.clone());
        }
        assert!(gossip_table.finished.contains(&data_id));

        // Time the finished data out and check it has been purged.
        Instant::advance_time(DEFAULT_FINISHED_ENTRY_DURATION_SECS * 1_000 + 1);
        gossip_table.purge_finished();
        assert!(!gossip_table.finished.contains(&data_id));

        // Add new complete data and pause.
        let _ = gossip_table.new_complete_data(&data_id, None);
        gossip_table.pause(&data_id);
        assert!(gossip_table.paused.contains_key(&data_id));

        // Time the paused data out and check it has been purged.
        Instant::advance_time(DEFAULT_FINISHED_ENTRY_DURATION_SECS * 1_000 + 1);
        gossip_table.purge_finished();
        assert!(!gossip_table.paused.contains_key(&data_id));
    }

    #[bench]
    fn benchmark_purging(bencher: &mut Bencher) {
        const ENTRY_COUNT: usize = 10_000;
        let mut rng = crate::new_rng();
        let node_ids = random_node_ids(&mut rng);
        let deploy_ids = iter::repeat_with(|| DeployHash::new(Digest::random(&mut rng)))
            .take(ENTRY_COUNT)
            .collect::<Vec<_>>();

        let mut gossip_table = GossipTable::new(Config::default());

        // Add new complete data and finish via infection limit.
        for deploy_id in &deploy_ids {
            let _ = gossip_table.new_complete_data(deploy_id, None);
            for node_id in &node_ids[0..EXPECTED_DEFAULT_INFECTION_TARGET] {
                let _ = gossip_table.we_infected(deploy_id, node_id.clone());
            }
            assert!(gossip_table.finished.contains(&deploy_id));
        }

        bencher.iter(|| gossip_table.purge_finished());
    }
}
