use core::fmt::{self, Display, Formatter};

use casper_types::{
    DeployHeader, EvmTransaction, InitiatorAddr, TimeDiff, Timestamp, Transaction, TransactionV1,
};
use datasize::DataSize;
use serde::Serialize;

/// Metadata carried by a version 1 transaction header.
#[derive(Debug, Clone, DataSize, PartialEq, Eq, Serialize)]
pub struct TransactionMetadata {
    initiator_addr: InitiatorAddr,
    timestamp: Timestamp,
    ttl: TimeDiff,
}

impl TransactionMetadata {
    /// Returns the transaction initiator.
    pub fn initiator_addr(&self) -> &InitiatorAddr {
        &self.initiator_addr
    }

    /// Returns the transaction timestamp.
    pub fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    /// Returns the transaction's time to live.
    pub fn ttl(&self) -> TimeDiff {
        self.ttl
    }
}

impl Display for TransactionMetadata {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "transaction-metadata[initiator_addr: {}]",
            self.initiator_addr,
        )
    }
}

impl Display for EvmTransactionMetadata {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "transaction-metadata[initiator_addr: {}]",
            self.initiator_addr,
        )
    }
}

/// Metadata carried by an EVM transaction header.
#[derive(Debug, Clone, DataSize, PartialEq, Eq, Serialize)]
pub struct EvmTransactionMetadata {
    initiator_addr: InitiatorAddr,
    timestamp: Timestamp,
    ttl: TimeDiff,
}

impl EvmTransactionMetadata {
    /// Returns the transaction initiator.
    pub fn initiator_addr(&self) -> &InitiatorAddr {
        &self.initiator_addr
    }

    /// Returns the transaction timestamp.
    pub fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    /// Returns the transaction's time to live.
    pub fn ttl(&self) -> TimeDiff {
        self.ttl
    }
}

/// A versioned wrapper for a transaction header or deploy header.
#[derive(Debug, Clone, DataSize, Serialize, PartialEq, Eq)]
pub enum TransactionHeader {
    /// A legacy deploy header.
    Deploy(DeployHeader),
    /// A version 1 transaction header.
    V1(TransactionMetadata),
    /// An EVM transaction header.
    Evm(EvmTransactionMetadata),
}

impl From<DeployHeader> for TransactionHeader {
    fn from(header: DeployHeader) -> Self {
        Self::Deploy(header)
    }
}

impl From<&TransactionV1> for TransactionHeader {
    fn from(transaction_v1: &TransactionV1) -> Self {
        let meta = TransactionMetadata {
            initiator_addr: transaction_v1.initiator_addr().clone(),
            timestamp: transaction_v1.timestamp(),
            ttl: transaction_v1.ttl(),
        };
        Self::V1(meta)
    }
}

impl From<&EvmTransaction> for TransactionHeader {
    fn from(transaction: &EvmTransaction) -> Self {
        let meta = EvmTransactionMetadata {
            initiator_addr: transaction.initiator_addr(),
            timestamp: transaction.timestamp(),
            ttl: transaction.ttl(),
        };
        Self::Evm(meta)
    }
}

impl From<&Transaction> for TransactionHeader {
    fn from(transaction: &Transaction) -> Self {
        match transaction {
            Transaction::Deploy(deploy) => deploy.header().clone().into(),
            Transaction::V1(v1) => v1.into(),
            Transaction::Evm(evm) => evm.as_ref().into(),
        }
    }
}

impl Display for TransactionHeader {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            TransactionHeader::Deploy(header) => Display::fmt(header, formatter),
            TransactionHeader::V1(meta) => Display::fmt(meta, formatter),
            TransactionHeader::Evm(meta) => Display::fmt(meta, formatter),
        }
    }
}
