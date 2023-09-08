use std::{
    cmp::{Ord, PartialOrd},
    fmt::{self, Display, Formatter},
    hash::Hash,
};

use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
use serde::{Deserialize, Serialize};

#[cfg(any(feature = "testing", test))]
use casper_types::{testing::TestRng, DeployApproval};
use casper_types::{DeployHash, PublicKey};

use crate::types::{DeployHashWithApprovals, DeployOrTransferHash};

/// The piece of information that will become the content of a future block (isn't finalized or
/// executed yet)
///
/// From the view of the consensus protocol this is the "consensus value": The protocol deals with
/// finalizing an order of `BlockPayload`s. Only after consensus has been reached, the block's
/// deploys actually get executed, and the executed block gets signed.
#[derive(
    Clone, DataSize, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize, Default,
)]
pub(crate) struct BlockPayload {
    deploys: Vec<DeployHashWithApprovals>,
    transfers: Vec<DeployHashWithApprovals>,
    accusations: Vec<PublicKey>,
    random_bit: bool,
}

impl BlockPayload {
    pub(crate) fn new(
        deploys: Vec<DeployHashWithApprovals>,
        transfers: Vec<DeployHashWithApprovals>,
        accusations: Vec<PublicKey>,
        random_bit: bool,
    ) -> Self {
        BlockPayload {
            deploys,
            transfers,
            accusations,
            random_bit,
        }
    }

    /// The list of deploys included in the block, excluding transfers.
    pub(crate) fn deploys(&self) -> &Vec<DeployHashWithApprovals> {
        &self.deploys
    }

    /// The list of transfers included in the block.
    pub(crate) fn transfers(&self) -> &Vec<DeployHashWithApprovals> {
        &self.transfers
    }

    /// Returns the set of validators that are reported as faulty in this block.
    pub(crate) fn accusations(&self) -> &Vec<PublicKey> {
        &self.accusations
    }

    pub(crate) fn random_bit(&self) -> bool {
        self.random_bit
    }

    /// An iterator over deploy hashes included in the block, excluding transfers.
    pub(crate) fn deploy_hashes(&self) -> impl Iterator<Item = &DeployHash> + Clone {
        self.deploys.iter().map(|dwa| dwa.deploy_hash())
    }

    /// An iterator over transfer hashes included in the block.
    pub(crate) fn transfer_hashes(&self) -> impl Iterator<Item = &DeployHash> + Clone {
        self.transfers.iter().map(|dwa| dwa.deploy_hash())
    }

    /// The list of deploy hashes chained with the list of transfer hashes.
    pub fn deploy_and_transfer_hashes(&self) -> impl Iterator<Item = &DeployHash> {
        self.deploy_hashes().chain(self.transfer_hashes())
    }

    /// Returns an iterator over all deploys and transfers.
    pub(crate) fn deploys_and_transfers_iter(
        &self,
    ) -> impl Iterator<Item = DeployOrTransferHash> + '_ {
        self.deploy_hashes()
            .copied()
            .map(DeployOrTransferHash::Deploy)
            .chain(
                self.transfer_hashes()
                    .copied()
                    .map(DeployOrTransferHash::Transfer),
            )
    }
}

impl Display for BlockPayload {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let count = self.deploys.len() + self.transfers.len();
        write!(formatter, "payload: {} deploys", count,)?;
        if !self.accusations.is_empty() {
            write!(formatter, ", {} accusations", self.accusations.len())?;
        }
        Ok(())
    }
}

#[cfg(any(feature = "testing", test))]
impl BlockPayload {
    #[allow(unused)] // TODO: remove when used in tests
    pub fn random(
        rng: &mut TestRng,
        num_deploys: usize,
        num_transfers: usize,
        num_approvals: usize,
        num_accusations: usize,
    ) -> Self {
        let mut total_approvals_left = num_approvals;
        const MAX_APPROVALS_PER_DEPLOY: usize = 100;

        let deploys = (0..num_deploys)
            .map(|n| {
                // We need at least one approval, and at least as many so that we are able to split
                // all the remaining approvals between the remaining deploys while not exceeding
                // the limit per deploy.
                let min_approval_count = total_approvals_left
                    .saturating_sub(
                        MAX_APPROVALS_PER_DEPLOY * (num_transfers + num_deploys - n - 1),
                    )
                    .max(1);
                // We have to leave at least one approval per deploy for the remaining deploys.
                let max_approval_count = MAX_APPROVALS_PER_DEPLOY
                    .min(total_approvals_left - (num_transfers + num_deploys - n - 1));
                let n_approvals = rng.gen_range(min_approval_count..=max_approval_count);
                total_approvals_left -= n_approvals;
                DeployHashWithApprovals::new(
                    DeployHash::random(rng),
                    (0..n_approvals)
                        .map(|_| DeployApproval::random(rng))
                        .collect(),
                )
            })
            .collect();

        let transfers = (0..num_transfers)
            .map(|n| {
                // We need at least one approval, and at least as many so that we are able to split
                // all the remaining approvals between the remaining transfers while not exceeding
                // the limit per deploy.
                let min_approval_count = total_approvals_left
                    .saturating_sub(MAX_APPROVALS_PER_DEPLOY * (num_transfers - n - 1))
                    .max(1);
                // We have to leave at least one approval per transfer for the remaining transfers.
                let max_approval_count =
                    MAX_APPROVALS_PER_DEPLOY.min(total_approvals_left - (num_transfers - n - 1));
                let n_approvals = rng.gen_range(min_approval_count..=max_approval_count);
                total_approvals_left -= n_approvals;
                DeployHashWithApprovals::new(
                    DeployHash::random(rng),
                    (0..n_approvals)
                        .map(|_| DeployApproval::random(rng))
                        .collect(),
                )
            })
            .collect();

        let accusations = (0..num_accusations)
            .map(|_| PublicKey::random(rng))
            .collect();

        Self {
            deploys,
            transfers,
            accusations,
            random_bit: rng.gen(),
        }
    }
}
