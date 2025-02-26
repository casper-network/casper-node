use std::{
    cmp::{Ord, PartialOrd},
    collections::{BTreeMap, BTreeSet},
    fmt::{self, Display, Formatter},
    hash::Hash,
};

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::{
    Approval, PublicKey, RewardedSignatures, TransactionHash, AUCTION_LANE_ID,
    INSTALL_UPGRADE_LANE_ID, MINT_LANE_ID,
};

/// The piece of information that will become the content of a future block (isn't finalized or
/// executed yet)
///
/// From the view of the consensus protocol this is the "consensus value": The protocol deals with
/// finalizing an order of `BlockPayload`s. Only after consensus has been reached, the block's
/// transactions actually get executed, and the executed block gets signed.
#[derive(Clone, DataSize, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockPayload {
    transactions: BTreeMap<u8, Vec<(TransactionHash, BTreeSet<Approval>)>>,
    accusations: Vec<PublicKey>,
    rewarded_signatures: RewardedSignatures,
    random_bit: bool,
    current_gas_price: u8,
}

impl Default for BlockPayload {
    fn default() -> Self {
        Self {
            transactions: Default::default(),
            accusations: vec![],
            rewarded_signatures: Default::default(),
            random_bit: false,
            current_gas_price: 1u8,
        }
    }
}

impl BlockPayload {
    pub(crate) fn new(
        transactions: BTreeMap<u8, Vec<(TransactionHash, BTreeSet<Approval>)>>,
        accusations: Vec<PublicKey>,
        rewarded_signatures: RewardedSignatures,
        random_bit: bool,
        current_gas_price: u8,
    ) -> Self {
        BlockPayload {
            transactions,
            accusations,
            rewarded_signatures,
            random_bit,
            current_gas_price,
        }
    }

    /// Returns the hashes and approvals of the mint transactions within the block.
    pub fn mint(&self) -> impl Iterator<Item = &(TransactionHash, BTreeSet<Approval>)> {
        let mut ret = vec![];
        if let Some(transactions) = self.transactions.get(&MINT_LANE_ID) {
            for transaction in transactions {
                ret.push(transaction);
            }
        }
        ret.into_iter()
    }

    /// Returns the hashes and approvals of the auction transactions within the block.
    pub fn auction(&self) -> impl Iterator<Item = &(TransactionHash, BTreeSet<Approval>)> {
        let mut ret = vec![];
        if let Some(transactions) = self.transactions.get(&AUCTION_LANE_ID) {
            for transaction in transactions {
                ret.push(transaction);
            }
        }
        ret.into_iter()
    }

    /// Returns the hashes and approvals of the installer / upgrader transactions within the block.
    pub fn install_upgrade(&self) -> impl Iterator<Item = &(TransactionHash, BTreeSet<Approval>)> {
        let mut ret = vec![];
        if let Some(transactions) = self.transactions.get(&INSTALL_UPGRADE_LANE_ID) {
            for transaction in transactions {
                ret.push(transaction);
            }
        }
        ret.into_iter()
    }

    /// Returns all the transaction hashes and approvals within the block by lane.
    pub fn transactions_by_lane(
        &self,
        lane: u8,
    ) -> impl Iterator<Item = &(TransactionHash, BTreeSet<Approval>)> {
        let mut ret = vec![];
        if let Some(transactions) = self.transactions.get(&lane) {
            for transaction in transactions {
                ret.push(transaction);
            }
        }
        ret.into_iter()
    }

    pub(crate) fn finalized_payload(&self) -> BTreeMap<u8, Vec<TransactionHash>> {
        let mut ret = BTreeMap::new();
        for (category, transactions) in &self.transactions {
            let transactions = transactions.iter().map(|(tx, _)| *tx).collect();
            ret.insert(*category, transactions);
        }

        ret
    }

    /// Returns true if even 1 transaction is in a lane other than supported.
    pub fn has_transaction_in_unsupported_lane(&self, supported_lanes: &[u8]) -> bool {
        // for all transaction lanes, if any of them are not in supported_lanes, true
        self.transactions
            .keys()
            .any(|lane_id| !supported_lanes.contains(lane_id))
    }

    /// Returns count of transactions by category.
    pub fn count(&self, lane: Option<u8>) -> usize {
        match lane {
            None => self.transactions.values().map(Vec::len).sum(),
            Some(lane) => match self.transactions.get(&lane) {
                Some(values) => values.len(),
                None => 0,
            },
        }
    }

    /// Returns all the transaction hashes and approvals within the block.
    pub fn all_transactions(&self) -> impl Iterator<Item = &(TransactionHash, BTreeSet<Approval>)> {
        self.transactions.values().flatten()
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

    /// The current gas price to execute the payload against.
    pub(crate) fn current_gas_price(&self) -> u8 {
        self.current_gas_price
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
