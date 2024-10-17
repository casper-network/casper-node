use casper_types::{DeployHeader, InitiatorAddr, TimeDiff, Timestamp, Transaction, TransactionV1};
use core::fmt::{self, Display, Formatter};
use datasize::DataSize;
use serde::Serialize;

#[derive(Debug, Clone, DataSize, PartialEq, Eq, Serialize)]
pub(crate) struct TransactionV1Metadata {
    initiator_addr: InitiatorAddr,
    timestamp: Timestamp,
    ttl: TimeDiff,
}

impl TransactionV1Metadata {
    pub(crate) fn initiator_addr(&self) -> &InitiatorAddr {
        &self.initiator_addr
    }

    pub(crate) fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    pub(crate) fn ttl(&self) -> TimeDiff {
        self.ttl
    }
}

impl Display for TransactionV1Metadata {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "transaction-v1-metadata[initiator_addr: {}]",
            self.initiator_addr,
        )
    }
}

#[derive(Debug, Clone, DataSize, Serialize, PartialEq, Eq)]
/// A versioned wrapper for a transaction header or deploy header.
pub(crate) enum TransactionHeader {
    Deploy(DeployHeader),
    V1(TransactionV1Metadata),
}

impl From<DeployHeader> for TransactionHeader {
    fn from(header: DeployHeader) -> Self {
        Self::Deploy(header)
    }
}

impl From<&TransactionV1> for TransactionHeader {
    fn from(transaction_v1: &TransactionV1) -> Self {
        let meta = TransactionV1Metadata {
            initiator_addr: transaction_v1.initiator_addr().clone(),
            timestamp: transaction_v1.timestamp(),
            ttl: transaction_v1.ttl(),
        };
        Self::V1(meta)
    }
}

impl From<&Transaction> for TransactionHeader {
    fn from(transaction: &Transaction) -> Self {
        match transaction {
            Transaction::Deploy(deploy) => deploy.header().clone().into(),
            Transaction::V1(v1) => v1.into(),
        }
    }
}

impl Display for TransactionHeader {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            TransactionHeader::Deploy(header) => Display::fmt(header, formatter),
            TransactionHeader::V1(meta) => Display::fmt(meta, formatter),
        }
    }
}
