// TODO - remove lint-relaxation once module is used.
#![allow(unused)]

#[cfg(not(test))]
use std::time::Instant;
use std::{
    collections::{hash_map::Entry, HashMap, HashSet},
    hash::Hash,
    time::Duration,
};

#[cfg(test)]
use fake_instant::FakeClock as Instant;

use serde::{
    de::{Deserializer, Error as SerdeError, Unexpected},
    Deserialize, Serialize,
};
use thiserror::Error;
use tracing::error;

use crate::small_network::NodeId;

const DEFAULT_INFECTION_TARGET: u8 = 3;
const DEFAULT_SATURATION_LIMIT_PERCENT: u8 = 80;
const MAX_SATURATION_LIMIT_PERCENT: u8 = 99;
const DEFAULT_FINISHED_ENTRY_DURATION_SECS: u64 = 3_600;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum GossipAction {
    /// This is new data, previously unknown by us, and for which we don't yet hold everything
    /// required to allow us start gossiping it onwards.  We should get the remaining parts from
    /// the sender and not gossip the ID onwards yet.
    GetRemainder,
    /// This is data already known to us, but for which we don't yet hold everything required to
    /// allow us start gossiping it onwards.  We should already be getting the remaining parts from
    /// a holder, so there's no need to do anything else now.
    AwaitingRemainder,
    /// We hold the data locally and should gossip the ID onwards.
    ShouldGossip { exclude_peers: HashSet<NodeId> },
    /// We hold the data locally, and we shouldn't gossip the ID onwards.
    Noop,
}

/// Error returned by a `GossipTable`.
#[derive(Debug, Error)]
pub enum Error {
    /// Invalid configuration value for `saturation_limit_percent`.
    #[error(
        "invalid saturation_limit_percent - should be between 0 and {} inclusive",
        MAX_SATURATION_LIMIT_PERCENT
    )]
    InvalidSaturationLimit,

    /// Attempted to force-finish or reset data which had already finished.
    #[error("data has already finished being gossiped")]
    AlreadyFinished,

    /// Attempted to reset data which had not been force-finished.
    #[error("data has not previously been force-finished")]
    NotForceFinished,
}

/// Configuration options for gossiping.
#[derive(Copy, Clone, Debug, Deserialize, Serialize)]
pub struct Config {
    /// Target number of peers to infect with a given piece of data.
    infection_target: u8,
    /// The saturation limit as a percentage, with a maximum value of 99.  Used as a termination
    /// condition.
    ///
    /// Example: assume the `infection_target` is 3, the `saturation_limit_percent` is 80, and we
    /// don't manage to newly infect 3 peers.  We will stop gossiping once we know of more than 15
    /// holders excluding us since 80% saturation would imply 3 new infections in 15 peers.
    #[serde(deserialize_with = "deserialize_saturation_limit_percent")]
    saturation_limit_percent: u8,
    /// The maximum duration for which to keep finished entries.
    ///
    /// The longer they are retained, the lower the likelihood of re-gossiping a piece of data.
    /// However, the longer they are retained, the larger the list of finished entries can grow.
    finished_entry_duration: Duration,
}

impl Config {
    pub(crate) fn new(
        infection_target: u8,
        saturation_limit_percent: u8,
        finished_entry_duration: Duration,
    ) -> Result<Self, Error> {
        if saturation_limit_percent > MAX_SATURATION_LIMIT_PERCENT {
            return Err(Error::InvalidSaturationLimit);
        }
        Ok(Config {
            infection_target,
            saturation_limit_percent,
            finished_entry_duration,
        })
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            infection_target: DEFAULT_INFECTION_TARGET,
            saturation_limit_percent: DEFAULT_SATURATION_LIMIT_PERCENT,
            finished_entry_duration: Duration::from_secs(DEFAULT_FINISHED_ENTRY_DURATION_SECS),
        }
    }
}

#[derive(Debug, Default)]
struct State {
    /// The peers excluding us which hold the data.
    holders: HashSet<NodeId>,
    /// Whether we hold the full data locally yet or not.
    held_by_us: bool,
    /// The subset of `holders` we have infected.  Not just a count so we don't attribute the same
    /// peer multiple times.
    infected_by_us: HashSet<NodeId>,
    /// Whether this entry has been "force-finished".  This allows us to only finish gossiping for
    /// this piece of data once - we can't keep toggling between force-finished and reset.
    force_finished: bool,
}

impl State {
    /// Returns whether we should finish gossiping this data.
    fn is_finished(&self, infection_target: u8, holders_limit: usize) -> bool {
        self.infected_by_us.len() >= infection_target as usize
            || self.holders.len() >= holders_limit
    }

    /// Returns a `GossipAction` derived from the given state.
    fn action(&self, infection_target: u8, holders_limit: usize, is_new: bool) -> GossipAction {
        if self.is_finished(infection_target, holders_limit) {
            return GossipAction::Noop;
        }

        if self.held_by_us {
            return GossipAction::ShouldGossip {
                exclude_peers: self.holders.clone(),
            };
        }

        if is_new {
            GossipAction::GetRemainder
        } else {
            GossipAction::AwaitingRemainder
        }
    }
}

#[derive(Debug)]
pub(crate) struct GossipTable<T> {
    /// Data IDs for which gossiping is still ongoing.
    current: HashMap<T, State>,
    /// Data IDs for which gossiping is complete.  The map's values are the times after which the
    /// relevant entries can be removed.
    finished: HashMap<T, Instant>,
    /// Data IDs for which gossiping has been force-finished (likely due to detecting that the data
    /// was not correct as per our current knowledge).  Such data could later be decided as still
    /// requiring to be gossiped, so we retain the `State` part here in order to resume gossiping.
    force_finished: HashMap<T, (State, Instant)>,
    /// See `Config::infection_target`.
    infection_target: u8,
    /// Derived from `Config::saturation_limit_percent` - we gossip data while the number of
    /// holders doesn't exceed `holders_limit`.
    holders_limit: usize,
    /// See `Config::finished_entry_duration`.
    finished_entry_duration: Duration,
}

impl<T: Copy + Eq + Hash> GossipTable<T> {
    /// Returns a new `GossipTable` using the provided configuration.
    pub(crate) fn new(config: Config) -> Self {
        let holders_limit = (100 * usize::from(config.infection_target))
            / (100 - usize::from(config.saturation_limit_percent));
        GossipTable {
            current: HashMap::new(),
            finished: HashMap::new(),
            force_finished: HashMap::new(),
            infection_target: config.infection_target,
            holders_limit,
            finished_entry_duration: config.finished_entry_duration,
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

        if self.finished.contains_key(data_id) {
            return GossipAction::Noop;
        }

        if let Some((state, _timeout)) = self.force_finished.get_mut(data_id) {
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
    /// its ID should be passed in `from`.  If received from a client or generated on this node,
    /// `maybe_holder` should be `None`.
    ///
    /// This should only be called once we hold everything locally we need to be able to gossip it
    /// onwards.  If we aren't able to gossip this data yet, call `new_data_id` instead.
    ///
    /// Returns whether we should gossip it, and a list of peers to exclude.
    pub(crate) fn new_complete_data(
        &mut self,
        data_id: &T,
        maybe_holder: Option<NodeId>,
    ) -> GossipAction {
        self.purge_finished();

        if self.finished.contains_key(data_id) {
            return GossipAction::Noop;
        }

        let update = |state: &mut State| {
            if let Some(holder) = maybe_holder {
                let _ = state.holders.insert(holder);
            }
            state.held_by_us = true;
        };

        if let Some((state, _timeout)) = self.force_finished.get_mut(data_id) {
            update(state);
            return GossipAction::Noop;
        }

        match self.current.entry(*data_id) {
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
        }
    }

    pub(crate) fn holders(&self, data_id: &T) -> Option<impl Iterator<Item = &NodeId>> {
        self.current
            .get(data_id)
            .or_else(|| {
                self.force_finished
                    .get(data_id)
                    .map(|(state, _timeout)| state)
            })
            .map(|state| state.holders.iter())
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
        self.purge_finished();

        let infection_target = self.infection_target;
        let holders_limit = self.holders_limit;
        let update = |state: &mut State| {
            let _ = state.holders.insert(peer);
            if by_us {
                let _ = state.infected_by_us.insert(peer);
            }
            state.is_finished(infection_target, holders_limit)
        };

        let is_finished = if let Some(state) = self.current.get_mut(data_id) {
            let is_finished = update(state);
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
            let _ = self.finished.insert(*data_id, timeout);
        }

        let is_finished = if let Some((state, _timeout)) = self.force_finished.get_mut(data_id) {
            update(state)
        } else {
            false
        };

        if is_finished {
            let _ = self.force_finished.remove(data_id);
            let timeout = Instant::now() + self.finished_entry_duration;
            let _ = self.finished.insert(*data_id, timeout);
        }

        GossipAction::Noop
    }

    /// We have deemed the data not suitable for gossiping further.
    ///
    /// Returns an error if gossiping this data was previously finished, either naturally or via a
    /// prior call to `force_finish`.
    pub(crate) fn force_finish(&mut self, data_id: &T) -> Result<(), Error> {
        self.purge_finished();

        if let Some(mut state) = self.current.remove(data_id) {
            if !state.force_finished {
                state.force_finished = true;
                let timeout = Instant::now() + self.finished_entry_duration;
                let _ = self.force_finished.insert(*data_id, (state, timeout));
                return Ok(());
            }
        }

        Err(Error::AlreadyFinished)
    }

    /// We have deemed invalid data (flagged via calling `force_finish`) as needing to be gossiped
    /// onwards.
    ///
    /// Returns an error if gossiping this data is not in a force-finished state.
    pub(crate) fn reset(&mut self, data_id: &T) -> Result<(), Error> {
        self.purge_finished();

        if let Some((state, _timeout)) = self.force_finished.remove(data_id) {
            let _ = self.current.insert(*data_id, state);
            return Ok(());
        }

        Err(Error::NotForceFinished)
    }

    /// Retain only those finished entries which still haven't timed out.
    fn purge_finished(&mut self) {
        let now = Instant::now();
        self.finished = self
            .finished
            .drain()
            .filter(|(_, timeout)| *timeout > now)
            .collect();
        self.force_finished = self
            .force_finished
            .drain()
            .filter(|(_, (_, timeout))| *timeout > now)
            .collect();
    }
}

/// Deserializes a `usize` but fails if it's not in the range 0..100.
fn deserialize_saturation_limit_percent<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: Deserializer<'de>,
{
    let saturation_limit_percent = u8::deserialize(deserializer)?;
    if saturation_limit_percent > MAX_SATURATION_LIMIT_PERCENT {
        error!(
            "saturation_limit_percent of {} is above {}",
            saturation_limit_percent, MAX_SATURATION_LIMIT_PERCENT
        );
        return Err(SerdeError::invalid_value(
            Unexpected::Unsigned(saturation_limit_percent as u64),
            &"a value between 0 and 99 inclusive",
        ));
    }

    Ok(saturation_limit_percent)
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, iter};

    use rand::Rng;

    use super::*;
    use crate::utils::DisplayIter;

    const EXPECTED_DEFAULT_INFECTION_TARGET: usize = 3;
    const EXPECTED_DEFAULT_HOLDERS_LIMIT: usize = 15;

    #[test]
    fn invalid_config_should_fail() {
        // saturation_limit_percent > MAX_SATURATION_LIMIT_PERCENT
        let finished_entry_duration = Duration::from_secs(DEFAULT_FINISHED_ENTRY_DURATION_SECS);
        let invalid_config = Config {
            infection_target: 3,
            saturation_limit_percent: MAX_SATURATION_LIMIT_PERCENT + 1,
            finished_entry_duration,
        };

        // Parsing should fail.
        let config_as_json = serde_json::to_string(&invalid_config).unwrap();
        assert!(serde_json::from_str::<Config>(&config_as_json).is_err());

        // Construction should fail.
        assert!(Config::new(3, MAX_SATURATION_LIMIT_PERCENT + 1, finished_entry_duration).is_err())
    }

    fn random_node_ids() -> Vec<NodeId> {
        let mut rng = rand::thread_rng();
        iter::repeat_with(|| rng.gen::<NodeId>())
            .take(EXPECTED_DEFAULT_HOLDERS_LIMIT + 3)
            .collect()
    }

    fn check_holders(expected: &[NodeId], gossip_table: &GossipTable<u64>, data_id: &u64) {
        let expected: BTreeSet<_> = expected.iter().collect();
        let actual: BTreeSet<_> = gossip_table
            .holders(data_id)
            .map(|itr| itr.collect())
            .unwrap_or_else(BTreeSet::new);
        assert!(
            expected == actual,
            "\nexpected: {}\nactual:   {}\n",
            DisplayIter::new(expected.iter()),
            DisplayIter::new(actual.iter())
        );
    }

    #[test]
    fn new_partial_data() {
        let node_ids = random_node_ids();

        let mut rng = rand::thread_rng();
        let data_id: u64 = rng.gen();

        let mut gossip_table = GossipTable::new(Config::default());
        assert_eq!(
            EXPECTED_DEFAULT_INFECTION_TARGET,
            gossip_table.infection_target as usize
        );
        assert_eq!(EXPECTED_DEFAULT_HOLDERS_LIMIT, gossip_table.holders_limit);

        // Check new partial data causes `GetRemainder` to be returned.
        let action = gossip_table.new_partial_data(&data_id, node_ids[0]);
        assert_eq!(GossipAction::GetRemainder, action);
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

        // Force-finish the gossiping and check same partial data from third source causes `Noop` to
        // be returned and holders updated.
        gossip_table.force_finish(&data_id).unwrap();
        let action = gossip_table.new_partial_data(&data_id, node_ids[2]);
        assert_eq!(GossipAction::Noop, action);
        check_holders(&node_ids[..3], &gossip_table, &data_id);

        // Reset the data and check same partial data from fourth source causes `AwaitingRemainder`
        // to be returned and holders updated.
        gossip_table.reset(&data_id).unwrap();
        let action = gossip_table.new_partial_data(&data_id, node_ids[3]);
        assert_eq!(GossipAction::AwaitingRemainder, action);
        check_holders(&node_ids[..4], &gossip_table, &data_id);

        // Finish the gossip by reporting three infections, then check same partial data causes
        // `Noop` to be returned and holders cleared.
        let limit = 4 + EXPECTED_DEFAULT_INFECTION_TARGET;
        for node_id in &node_ids[4..limit] {
            let _ = gossip_table.we_infected(&data_id, *node_id);
        }
        let action = gossip_table.new_partial_data(&data_id, node_ids[limit]);
        assert_eq!(GossipAction::Noop, action);
        check_holders(&node_ids[..0], &gossip_table, &data_id);

        // Time the finished data out, then check same partial data causes `GetRemainder` to be
        // returned as per a completely new entry.
        Instant::advance_time(DEFAULT_FINISHED_ENTRY_DURATION_SECS * 1_000);
        let action = gossip_table.new_partial_data(&data_id, node_ids[0]);
        assert_eq!(GossipAction::GetRemainder, action);
        check_holders(&node_ids[..1], &gossip_table, &data_id);
    }

    #[test]
    fn new_complete_data() {
        let node_ids = random_node_ids();

        let mut rng = rand::thread_rng();
        let data_id: u64 = rng.gen();

        let mut gossip_table = GossipTable::new(Config::default());

        let should_gossip = |count: usize| GossipAction::ShouldGossip {
            exclude_peers: node_ids[..count].iter().copied().collect(),
        };

        // Check new complete data from us causes `ShouldGossip` to be returned.
        let action = gossip_table.new_complete_data(&data_id, None);
        assert_eq!(should_gossip(0), action);
        check_holders(&node_ids[..0], &gossip_table, &data_id);

        // Check same complete data from other source causes `ShouldGossip` to be returned and
        // holders updated.
        let action = gossip_table.new_complete_data(&data_id, Some(node_ids[0]));
        assert_eq!(should_gossip(1), action);
        check_holders(&node_ids[..1], &gossip_table, &data_id);

        // Force-finish the gossiping and check same complete data from third source causes `Noop`
        // to be returned and holders updated.
        gossip_table.force_finish(&data_id).unwrap();
        let action = gossip_table.new_complete_data(&data_id, Some(node_ids[1]));
        assert_eq!(GossipAction::Noop, action);
        check_holders(&node_ids[..2], &gossip_table, &data_id);

        // Reset the data and check same complete data from third source causes `ShouldGossip` to be
        // returned and holders updated.
        gossip_table.reset(&data_id).unwrap();
        let action = gossip_table.new_complete_data(&data_id, Some(node_ids[2]));
        assert_eq!(should_gossip(3), action);
        check_holders(&node_ids[..3], &gossip_table, &data_id);

        // Finish the gossip by reporting enough non-infections, then check same complete data
        // causes `Noop` to be returned and holders cleared.
        for node_id in &node_ids[3..] {
            let _ = gossip_table.we_infected(&data_id, *node_id);
        }
        let action = gossip_table.new_complete_data(&data_id, None);
        assert_eq!(GossipAction::Noop, action);
        check_holders(&node_ids[..0], &gossip_table, &data_id);

        // Time the finished data out, then check same complete data causes `ShouldGossip` to be
        // returned as per a completely new entry.
        Instant::advance_time(DEFAULT_FINISHED_ENTRY_DURATION_SECS * 1_000);
        let action = gossip_table.new_complete_data(&data_id, Some(node_ids[0]));
        assert_eq!(should_gossip(1), action);
        check_holders(&node_ids[..1], &gossip_table, &data_id);
    }

    #[test]
    fn should_terminate_via_infection_limit() {
        let node_ids = random_node_ids();

        let mut rng = rand::thread_rng();
        let data_id: u64 = rng.gen();

        let mut gossip_table = GossipTable::new(Config::default());

        let should_gossip = |count: usize| GossipAction::ShouldGossip {
            exclude_peers: node_ids[..count].iter().copied().collect(),
        };

        // Add new complete data from us and check two infections doesn't cause us to stop
        // gossiping.
        let _ = gossip_table.new_complete_data(&data_id, None);
        let limit = EXPECTED_DEFAULT_INFECTION_TARGET - 1;
        for (index, node_id) in node_ids.iter().enumerate().take(limit) {
            let action = gossip_table.we_infected(&data_id, *node_id);
            assert_eq!(should_gossip(index + 1), action);
        }

        // Check recording an infection from an already-recorded infectee doesn't cause us to stop
        // gossiping.
        let action = gossip_table.we_infected(&data_id, node_ids[limit - 1]);
        assert_eq!(should_gossip(limit), action);

        // Check third new infection does cause us to stop gossiping.
        let action = gossip_table.we_infected(&data_id, node_ids[limit]);
        assert_eq!(GossipAction::Noop, action);
    }

    #[test]
    fn should_terminate_via_saturation() {
        let node_ids = random_node_ids();

        let mut rng = rand::thread_rng();
        let data_id: u64 = rng.gen();

        let mut gossip_table = GossipTable::new(Config::default());

        let should_gossip = |count: usize| GossipAction::ShouldGossip {
            exclude_peers: node_ids[..count].iter().copied().collect(),
        };

        // Add new complete data with 14 non-infections and check this doesn't cause us to stop
        // gossiping.
        let _ = gossip_table.new_complete_data(&data_id, None);
        let limit = EXPECTED_DEFAULT_HOLDERS_LIMIT - 1;
        for (index, node_id) in node_ids.iter().enumerate().take(limit) {
            let action = gossip_table.already_infected(&data_id, *node_id);
            assert_eq!(should_gossip(index + 1), action);
        }

        // Check recording a non-infection from an already-recorded holder doesn't cause us to stop
        // gossiping.
        let action = gossip_table.already_infected(&data_id, node_ids[0]);
        assert_eq!(should_gossip(limit), action);

        // Check 15th non-infection does cause us to stop gossiping.
        let action = gossip_table.we_infected(&data_id, node_ids[limit]);
        assert_eq!(GossipAction::Noop, action);
    }

    #[test]
    fn should_not_terminate_below_infection_limit_and_saturation() {
        let node_ids = random_node_ids();

        let mut rng = rand::thread_rng();
        let data_id: u64 = rng.gen();

        let mut gossip_table = GossipTable::new(Config::default());

        let should_gossip = |count: usize| GossipAction::ShouldGossip {
            exclude_peers: node_ids[..count].iter().copied().collect(),
        };

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
        assert_eq!(should_gossip(holders_limit + 1), action);
    }

    #[test]
    fn should_purge() {
        let node_ids = random_node_ids();

        let mut rng = rand::thread_rng();
        let data_id: u64 = rng.gen();

        let mut gossip_table = GossipTable::new(Config::default());

        // Add new complete data and finish via infection limit.
        let _ = gossip_table.new_complete_data(&data_id, None);
        for node_id in &node_ids[0..EXPECTED_DEFAULT_INFECTION_TARGET] {
            let _ = gossip_table.we_infected(&data_id, *node_id);
        }
        assert!(gossip_table.finished.contains_key(&data_id));

        // Time the finished data out and check it has been purged.
        Instant::advance_time(DEFAULT_FINISHED_ENTRY_DURATION_SECS * 1_000);
        gossip_table.purge_finished();
        assert!(!gossip_table.finished.contains_key(&data_id));

        // Add new complete data and force-finish.
        let _ = gossip_table.new_complete_data(&data_id, None);
        gossip_table.force_finish(&data_id).unwrap();
        assert!(gossip_table.force_finished.contains_key(&data_id));

        // Time the finished data out and check it has been purged.
        Instant::advance_time(DEFAULT_FINISHED_ENTRY_DURATION_SECS * 1_000);
        gossip_table.purge_finished();
        assert!(!gossip_table.force_finished.contains_key(&data_id));
    }
}
