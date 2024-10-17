use alloc::{collections::BTreeMap, vec::Vec};
use core::fmt::{self, Display, Formatter};
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "once_cell", test))]
use once_cell::sync::OnceCell;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    block::RewardedSignatures,
    bytesrepr::{self, FromBytes, ToBytes},
    Digest, TransactionHash, AUCTION_LANE_ID, INSTALL_UPGRADE_LANE_ID, LARGE_WASM_LANE_ID,
    MEDIUM_WASM_LANE_ID, MINT_LANE_ID, SMALL_WASM_LANE_ID,
};

/// The body portion of a block. Version 2.
#[derive(Clone, Eq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct BlockBodyV2 {
    /// Map of transactions mapping categories to a list of transaction hashes.
    pub(super) transactions: BTreeMap<u8, Vec<TransactionHash>>,
    /// List of identifiers for finality signatures for a particular past block.
    pub(super) rewarded_signatures: RewardedSignatures,
    #[serde(skip)]
    #[cfg_attr(
        all(any(feature = "once_cell", test), feature = "datasize"),
        data_size(skip)
    )]
    #[cfg(any(feature = "once_cell", test))]
    pub(super) hash: OnceCell<Digest>,
}

impl BlockBodyV2 {
    /// Constructs a new `BlockBodyV2`.
    pub(crate) fn new(
        transactions: BTreeMap<u8, Vec<TransactionHash>>,
        rewarded_signatures: RewardedSignatures,
    ) -> Self {
        BlockBodyV2 {
            transactions,
            rewarded_signatures,
            #[cfg(any(feature = "once_cell", test))]
            hash: OnceCell::new(),
        }
    }

    /// Returns the hashes of the transactions within the block filtered by lane_id.
    pub fn transaction_by_lane(&self, lane_id: u8) -> impl Iterator<Item = TransactionHash> {
        match self.transactions.get(&lane_id) {
            Some(transactions) => transactions.to_vec(),
            None => vec![],
        }
        .into_iter()
    }

    /// Returns the hashes of the mint transactions within the block.
    pub fn mint(&self) -> impl Iterator<Item = TransactionHash> {
        self.transaction_by_lane(MINT_LANE_ID)
    }

    /// Returns the hashes of the auction transactions within the block.
    pub fn auction(&self) -> impl Iterator<Item = TransactionHash> {
        self.transaction_by_lane(AUCTION_LANE_ID)
    }

    /// Returns the hashes of the installer/upgrader transactions within the block.
    pub fn install_upgrade(&self) -> impl Iterator<Item = TransactionHash> {
        self.transaction_by_lane(INSTALL_UPGRADE_LANE_ID)
    }

    /// Returns the hashes of the transactions filtered by lane id within the block.
    pub fn transactions_by_lane_id(&self, lane_id: u8) -> impl Iterator<Item = TransactionHash> {
        self.transaction_by_lane(lane_id)
    }

    /// Returns a reference to the collection of mapped transactions.
    pub fn transactions(&self) -> &BTreeMap<u8, Vec<TransactionHash>> {
        &self.transactions
    }

    /// Returns all of the transaction hashes in the order in which they were executed.
    pub fn all_transactions(&self) -> impl Iterator<Item = &TransactionHash> {
        self.transactions.values().flatten()
    }

    /// Returns the body hash, i.e. the hash of the body's serialized bytes.
    pub fn hash(&self) -> Digest {
        #[cfg(any(feature = "once_cell", test))]
        return *self.hash.get_or_init(|| self.compute_hash());

        #[cfg(not(any(feature = "once_cell", test)))]
        self.compute_hash()
    }

    fn compute_hash(&self) -> Digest {
        let serialized_body = self
            .to_bytes()
            .unwrap_or_else(|error| panic!("should serialize block body: {}", error));
        Digest::hash(serialized_body)
    }

    /// Return the list of identifiers for finality signatures for a particular past block.
    pub fn rewarded_signatures(&self) -> &RewardedSignatures {
        &self.rewarded_signatures
    }
}

impl PartialEq for BlockBodyV2 {
    fn eq(&self, other: &BlockBodyV2) -> bool {
        // Destructure to make sure we don't accidentally omit fields.
        #[cfg(any(feature = "once_cell", test))]
        let BlockBodyV2 {
            transactions,
            rewarded_signatures,
            hash: _,
        } = self;
        #[cfg(not(any(feature = "once_cell", test)))]
        let BlockBodyV2 {
            transactions,
            rewarded_signatures,
        } = self;
        *transactions == other.transactions && *rewarded_signatures == other.rewarded_signatures
    }
}

impl Display for BlockBodyV2 {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "block body, {} mint, {} auction, {} install_upgrade, {} large wasm, {} medium wasm, {} small wasm",
            self.mint().count(),
            self.auction().count(),
            self.install_upgrade().count(),
            self.transaction_by_lane(LARGE_WASM_LANE_ID).count(),
            self.transaction_by_lane(MEDIUM_WASM_LANE_ID).count(),
            self.transaction_by_lane(SMALL_WASM_LANE_ID).count(),
        )
    }
}

impl ToBytes for BlockBodyV2 {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.transactions.write_bytes(writer)?;
        self.rewarded_signatures.write_bytes(writer)?;
        Ok(())
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.transactions.serialized_length() + self.rewarded_signatures.serialized_length()
    }
}

impl FromBytes for BlockBodyV2 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (transactions, bytes) = FromBytes::from_bytes(bytes)?;
        let (rewarded_signatures, bytes) = RewardedSignatures::from_bytes(bytes)?;
        let body = BlockBodyV2 {
            transactions,
            rewarded_signatures,
            #[cfg(any(feature = "once_cell", test))]
            hash: OnceCell::new(),
        };
        Ok((body, bytes))
    }
}
