use std::{
    cmp::{Ord, PartialOrd},
    collections::{BTreeMap, BTreeSet},
    fmt::{self, Display, Formatter},
    hash::Hash,
};

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::{Approval, PublicKey, RewardedSignatures, TransactionCategory, TransactionHash};

/// The piece of information that will become the content of a future block (isn't finalized or
/// executed yet)
///
/// From the view of the consensus protocol this is the "consensus value": The protocol deals with
/// finalizing an order of `BlockPayload`s. Only after consensus has been reached, the block's
/// deploys actually get executed, and the executed block gets signed.
#[derive(
    Clone, DataSize, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize, Default,
)]
pub struct BlockPayload {
    transactions: BTreeMap<TransactionCategory, Vec<(TransactionHash, BTreeSet<Approval>)>>,
    accusations: Vec<PublicKey>,
    rewarded_signatures: RewardedSignatures,
    random_bit: bool,
}

impl BlockPayload {
    pub(crate) fn new(
        transactions: BTreeMap<TransactionCategory, Vec<(TransactionHash, BTreeSet<Approval>)>>,
        accusations: Vec<PublicKey>,
        rewarded_signatures: RewardedSignatures,
        random_bit: bool,
    ) -> Self {
        BlockPayload {
            transactions,
            accusations,
            rewarded_signatures,
            random_bit,
        }
    }

    /// Returns the hashes and approvals of the mint transactions within the block.
    pub fn mint(&self) -> impl Iterator<Item = &(TransactionHash, BTreeSet<Approval>)> {
        let mut ret = vec![];
        let _ = self.transactions.iter().map(|(k, v)| {
            if *k == TransactionCategory::Mint {
                ret.extend(v)
            }
        });
        ret.into_iter()
    }

    /// Returns the hashes and approvals of the auction transactions within the block.
    pub fn auction(&self) -> impl Iterator<Item = &(TransactionHash, BTreeSet<Approval>)> {
        let mut ret = vec![];
        let _ = self.transactions.iter().map(|(k, v)| {
            if *k == TransactionCategory::Auction {
                ret.extend(v)
            }
        });
        ret.into_iter()
    }

    /// Returns the hashes and approvals of the install / upgrade transactions within the block.
    pub fn install_upgrade(&self) -> impl Iterator<Item = &(TransactionHash, BTreeSet<Approval>)> {
        let mut ret = vec![];
        let _ = self.transactions.iter().map(|(k, v)| {
            if *k == TransactionCategory::InstallUpgrade {
                ret.extend(v)
            }
        });
        ret.into_iter()
    }

    /// Returns the hashes and approvals of the standard transactions within the block.
    pub fn standard(&self) -> impl Iterator<Item = &(TransactionHash, BTreeSet<Approval>)> {
        let mut ret = vec![];
        let _ = self.transactions.iter().map(|(k, v)| {
            if *k == TransactionCategory::Standard {
                ret.extend(v)
            }
        });
        ret.into_iter()
    }

    /// Returns count of transactions by category.
    pub fn count(&self, category: Option<TransactionCategory>) -> usize {
        match category {
            None => self.transactions.values().len(),
            Some(category) => match self.transactions.get(&category) {
                Some(values) => values.len(),
                None => 0,
            },
        }
    }

    /// Returns all of the transaction hashes and approvals within the block.
    pub fn all_transactions(&self) -> impl Iterator<Item = &(TransactionHash, BTreeSet<Approval>)> {
        let mut ret: Vec<&(TransactionHash, BTreeSet<Approval>)> = vec![];
        for v in self.transactions.values() {
            for val in v {
                ret.push(val)
            }
        }
        ret.into_iter()
    }

    /// Returns the set of validators that are reported as faulty in this block.
    pub(crate) fn accusations(&self) -> &Vec<PublicKey> {
        &self.accusations
    }

    pub(crate) fn random_bit(&self) -> bool {
        self.random_bit
    }

    /// The finality signatures for the past blocks that will be rewarded in this block.
    pub(crate) fn rewarded_signatures(&self) -> &RewardedSignatures {
        &self.rewarded_signatures
    }

    pub(crate) fn all_transaction_hashes(&self) -> impl Iterator<Item = TransactionHash> {
        let mut ret: Vec<TransactionHash> = vec![];
        for values in self.transactions.values() {
            for (transaction_hash, _) in values {
                ret.push(*transaction_hash);
            }
        }
        ret.into_iter()
    }
}

impl Display for BlockPayload {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let count = self.count(None);
        write!(formatter, "payload: {} txns", count)?;
        if !self.accusations.is_empty() {
            write!(formatter, ", {} accusations", self.accusations.len())?;
        }
        Ok(())
    }
}
