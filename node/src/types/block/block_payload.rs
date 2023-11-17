use std::{
    cmp::{Ord, PartialOrd},
    fmt::{self, Display, Formatter},
    hash::Hash,
};

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::{PublicKey, RewardedSignatures};

use crate::types::{TransactionHashWithApprovals, TypedTransactionHash};

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
    transfer: Vec<TransactionHashWithApprovals>,
    staking: Vec<TransactionHashWithApprovals>,
    install_upgrade: Vec<TransactionHashWithApprovals>,
    standard: Vec<TransactionHashWithApprovals>,
    accusations: Vec<PublicKey>,
    rewarded_signatures: RewardedSignatures,
    random_bit: bool,
}

impl BlockPayload {
    pub(crate) fn new(
        transfer: Vec<TransactionHashWithApprovals>,
        staking: Vec<TransactionHashWithApprovals>,
        install_upgrade: Vec<TransactionHashWithApprovals>,
        standard: Vec<TransactionHashWithApprovals>,
        accusations: Vec<PublicKey>,
        rewarded_signatures: RewardedSignatures,
        random_bit: bool,
    ) -> Self {
        BlockPayload {
            transfer,
            staking,
            install_upgrade,
            standard,
            accusations,
            rewarded_signatures,
            random_bit,
        }
    }

    /// Returns the hashes and approvals of the transfer transactions within the block.
    pub fn transfer(&self) -> impl Iterator<Item = &TransactionHashWithApprovals> {
        self.transfer.iter()
    }

    /// Returns the hashes and approvals of the non-transfer, native transactions within the block.
    pub fn staking(&self) -> impl Iterator<Item = &TransactionHashWithApprovals> {
        self.staking.iter()
    }

    /// Returns the hashes and approvals of the installer/upgrader userland transactions within the
    /// block.
    pub fn install_upgrade(&self) -> impl Iterator<Item = &TransactionHashWithApprovals> {
        self.install_upgrade.iter()
    }

    /// Returns the hashes and approvals of all other transactions within the block.
    pub fn standard(&self) -> impl Iterator<Item = &TransactionHashWithApprovals> {
        self.standard.iter()
    }

    /// Returns all of the transaction hashes and approvals within the block.
    pub fn all_transactions(&self) -> impl Iterator<Item = &TransactionHashWithApprovals> {
        self.transfer()
            .chain(self.staking())
            .chain(self.install_upgrade())
            .chain(self.standard())
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

    pub(crate) fn typed_transaction_hashes(
        &self,
    ) -> impl Iterator<Item = TypedTransactionHash> + '_ {
        let transfer = self
            .transfer
            .iter()
            .map(|thwa| TypedTransactionHash::Transfer(thwa.transaction_hash()));
        let staking = self
            .staking
            .iter()
            .map(|thwa| TypedTransactionHash::Staking(thwa.transaction_hash()));
        let install_upgrade = self
            .install_upgrade
            .iter()
            .map(|thwa| TypedTransactionHash::InstallUpgrade(thwa.transaction_hash()));
        let standard = self
            .standard
            .iter()
            .map(|thwa| TypedTransactionHash::Standard(thwa.transaction_hash()));
        transfer
            .chain(staking)
            .chain(install_upgrade)
            .chain(standard)
    }
}

impl Display for BlockPayload {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let count = self.transfer.len()
            + self.staking.len()
            + self.install_upgrade.len()
            + self.standard.len();
        write!(formatter, "payload: {} txns", count)?;
        if !self.accusations.is_empty() {
            write!(formatter, ", {} accusations", self.accusations.len())?;
        }
        Ok(())
    }
}
