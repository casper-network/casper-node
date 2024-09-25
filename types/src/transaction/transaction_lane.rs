#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// 1.x of the protocol had two implicit categories...standard and native transfer / mint
// 2.x and onwards the protocol has explicit categories.
// For legacy deploy support purposes, 1.x native transfers map to Mint, and all other deploys
// map to Standard.

// NOTE: there is a direct correlation between the block body structure and the transaction
// categories. A given block structure defines some number of lanes for transactions.
// Thus, a given transaction explicitly specifies which lane within the block
// structure it is intended to go into.

// Conceptually, the enum could just as easily be flipped around to be defined by the Block
// variant and be called something like BlockLane or BlockTransactionCategory, etc. It's only
// a matter of perspective.

use crate::{
    transaction::{deploy::DeployCategory, transaction_v1::TransactionLane as V1},
    Deploy,
};

/// The category of a [`Transaction`].
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "Session kind of a Transaction.")
)]
#[serde(deny_unknown_fields)]
#[repr(u8)]
pub(crate) enum TransactionLane {
    /// The supported categories of transactions. This was not explicit in protocol 1.x
    /// but was made explicit in protocol 2.x. Thus V1 is introduced in protocol 2.0
    /// Older deploys are retroactively mapped into the corresponding variants to
    /// allow retro-compatibility. Think of it as a retcon.
    V1(V1),
}

impl From<Deploy> for TransactionLane {
    fn from(value: Deploy) -> Self {
        // To hand waive away legacy issues, we just curry the implicit categories from protocol 1.x
        // forward to the corresponding protocol 2.x explicit categories.
        if value.is_transfer() {
            TransactionLane::V1(V1::Mint)
        } else {
            TransactionLane::V1(V1::Large)
        }
    }
}

impl From<DeployCategory> for TransactionLane {
    fn from(value: DeployCategory) -> Self {
        // To hand waive away legacy issues, we just curry the implicit categories from protocol 1.x
        // forward to the corresponding protocol 2.x explicit categories.
        match value {
            DeployCategory::Standard => TransactionLane::V1(V1::Large),
            DeployCategory::Transfer => TransactionLane::V1(V1::Mint),
        }
    }
}

impl From<V1> for TransactionLane {
    fn from(value: V1) -> Self {
        TransactionLane::V1(value)
    }
}
