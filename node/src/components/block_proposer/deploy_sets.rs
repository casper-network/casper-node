use std::{
    collections::HashMap,
    fmt::{self, Display, Formatter},
    hash::Hash,
};

use datasize::DataSize;

use super::{event::DeployInfo, BlockHeight, FinalizationQueue};
use crate::types::{DeployHash, DeployHeader, Timestamp};

/// Stores the internal state of the BlockProposer.
#[derive(Clone, DataSize, Debug, Default)]
pub(super) struct BlockProposerDeploySets {
    /// The collection of deploys pending for inclusion in a block, with a timestamp of when we
    /// received them.
    pub(super) pending_deploys: HashMap<DeployHash, (DeployInfo, Timestamp)>,
    /// The collection of transfers pending for inclusion in a block, with a timestamp of when we
    /// received them.
    pub(super) pending_transfers: HashMap<DeployHash, (DeployInfo, Timestamp)>,
    /// The deploys that have already been included in a finalized block.
    pub(super) finalized_deploys: HashMap<DeployHash, DeployHeader>,
    /// The next block height we expect to be finalized.
    /// If we receive a notification of finalization of a later block, we will store it in
    /// finalization_queue.
    /// If we receive a request that contains a later next_finalized, we will store it in
    /// request_queue.
    pub(super) next_finalized: BlockHeight,
    /// The queue of finalized block contents awaiting inclusion in `self.finalized_deploys`.
    pub(super) finalization_queue: FinalizationQueue,
}

impl BlockProposerDeploySets {
    /// Constructs the instance of `BlockProposerDeploySets` from the list of finalized deploys.
    pub(super) fn from_finalized(
        finalized_deploys: Vec<(DeployHash, DeployHeader)>,
        next_finalized_height: u64,
    ) -> BlockProposerDeploySets {
        BlockProposerDeploySets {
            finalized_deploys: finalized_deploys.into_iter().collect(),
            next_finalized: next_finalized_height,
            ..Default::default()
        }
    }
}

impl Display for BlockProposerDeploySets {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(
            f,
            "(pending:{}, finalized:{})",
            self.pending_deploys.len() + self.pending_transfers.len(),
            self.finalized_deploys.len()
        )
    }
}

impl BlockProposerDeploySets {
    /// Prunes expired deploy information from the BlockProposerState,  returns the
    /// hashes of deploys pruned.
    pub(crate) fn prune(&mut self, current_instant: Timestamp) -> Vec<DeployHash> {
        let pending_deploys = prune_pending_deploys(&mut self.pending_deploys, current_instant);
        let pending_transfers = prune_pending_deploys(&mut self.pending_transfers, current_instant);
        let finalized = prune_deploys(&mut self.finalized_deploys, current_instant);

        [pending_deploys, pending_transfers, finalized].concat()
    }
}

/// Drains items that satisfy the given predicate from the hash map and retains the rest.
/// Returns keys of the drained elements.
///
/// To be replaced with `HashMap::drain_filter` when stabilized.
/// [https://doc.rust-lang.org/std/collections/struct.HashMap.html#method.drain_filter]
fn hash_map_drain_filter_in_place<K, V, F>(hash_map: &mut HashMap<K, V>, pred: F) -> Vec<K>
where
    K: Eq + Hash + Copy,
    F: Fn(&V) -> bool,
{
    let mut drained = vec![];
    let retained: HashMap<_, _> = hash_map
        .drain()
        .filter_map(|(k, v)| pred(&v).then(|| drained.push(k)).is_none().then(|| (k, v)))
        .collect();

    hash_map.extend(retained);
    drained
}

/// Prunes expired deploy information from an individual deploy collection, returns the
/// hashes of deploys pruned.
pub(super) fn prune_deploys(
    deploys: &mut HashMap<DeployHash, DeployHeader>,
    current_instant: Timestamp,
) -> Vec<DeployHash> {
    hash_map_drain_filter_in_place(deploys, |header| header.expired(current_instant))
}

/// Prunes expired deploy information from an individual pending deploy collection, returns the
/// hashes of deploys pruned.
pub(super) fn prune_pending_deploys(
    deploys: &mut HashMap<DeployHash, (DeployInfo, Timestamp)>,
    current_instant: Timestamp,
) -> Vec<DeployHash> {
    hash_map_drain_filter_in_place(deploys, |(deploy_info, _)| {
        deploy_info.header.expired(current_instant)
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        testing::TestRng,
        types::{Deploy, TimeDiff},
    };

    use super::*;

    /// Creates a test deploy created at given instant and with given ttl.
    fn create_test_deploy(
        created_ago: TimeDiff,
        ttl: TimeDiff,
        now: Timestamp,
        mut test_rng: &mut TestRng,
    ) -> Deploy {
        Deploy::random_with_timestamp_and_ttl(&mut test_rng, now - created_ago, ttl)
    }

    /// Creates a random deploy that is considered expired.
    fn create_expired_deploy(now: Timestamp, test_rng: &mut TestRng) -> Deploy {
        create_test_deploy(
            TimeDiff::from_seconds(20),
            TimeDiff::from_seconds(10),
            now,
            test_rng,
        )
    }

    /// Creates a random deploy that is considered not expired.
    fn create_not_expired_deploy(now: Timestamp, test_rng: &mut TestRng) -> Deploy {
        create_test_deploy(
            TimeDiff::from_seconds(20),
            TimeDiff::from_seconds(60),
            now,
            test_rng,
        )
    }

    #[test]
    fn prunes_pending_deploys() {
        let mut test_rng = TestRng::new();
        let mut deploys: HashMap<DeployHash, (DeployInfo, Timestamp)> = HashMap::new();
        let now = Timestamp::now();

        let deploy_1 = create_not_expired_deploy(now, &mut test_rng);
        let deploy_2 = create_expired_deploy(now, &mut test_rng);
        let deploy_3 = create_expired_deploy(now, &mut test_rng);
        let deploy_4 = create_not_expired_deploy(now, &mut test_rng);
        let deploy_5 = create_expired_deploy(now, &mut test_rng);

        deploys.insert(*deploy_1.id(), (deploy_1.deploy_info().unwrap(), now));
        deploys.insert(*deploy_2.id(), (deploy_2.deploy_info().unwrap(), now));
        deploys.insert(*deploy_3.id(), (deploy_3.deploy_info().unwrap(), now));
        deploys.insert(*deploy_4.id(), (deploy_4.deploy_info().unwrap(), now));
        deploys.insert(*deploy_5.id(), (deploy_5.deploy_info().unwrap(), now));

        // We expect deploys created with `create_expired_deploy` to be drained
        let mut expected_drained = vec![*deploy_2.id(), *deploy_3.id(), *deploy_5.id()];
        expected_drained.sort();
        let mut drained = prune_pending_deploys(&mut deploys, now);
        drained.sort();
        assert_eq!(expected_drained, drained);

        // We expect deploys created with `create_not_expired_deploy` to be retained
        let mut expected_retained = vec![*deploy_1.id(), *deploy_4.id()];
        expected_retained.sort();
        let mut retained = deploys
            .into_iter()
            .map(|(deploy_hash, _)| deploy_hash)
            .collect::<Vec<_>>();
        retained.sort();
        assert_eq!(expected_retained, retained);
    }

    mod hash_map_drain_filter_in_place {
        use crate::components::block_proposer::deploy_sets::hash_map_drain_filter_in_place;

        #[test]
        fn returns_drained() {
            use std::collections::HashMap;
            let mut hash_map = HashMap::new();
            hash_map.insert("A", 1);
            hash_map.insert("B", 0);
            hash_map.insert("C", 1);
            hash_map.insert("D", 0);

            let mut drained = hash_map_drain_filter_in_place(&mut hash_map, |value| *value == 1);
            drained.sort_unstable();

            let expected_drained = vec!["A", "C"];
            assert_eq!(expected_drained, drained);

            let mut expected_retained = HashMap::new();
            expected_retained.insert("B", 0);
            expected_retained.insert("D", 0);

            assert_eq!(expected_retained, hash_map);
        }
    }
}
