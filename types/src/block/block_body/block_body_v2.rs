use alloc::vec::Vec;
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
    Digest, PublicKey, TransactionHash,
};

/// The body portion of a block. Version 2.
#[derive(Clone, Eq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct BlockBodyV2 {
    /// The public key of the validator which proposed the block.
    pub(super) proposer: PublicKey,
    /// The hashes of the transfer transactions within the block.
    pub(super) transfer: Vec<TransactionHash>,
    /// The hashes of the non-transfer, native transactions within the block.
    pub(super) staking: Vec<TransactionHash>,
    /// The hashes of the installer/upgrader transactions within the block.
    pub(super) install_upgrade: Vec<TransactionHash>,
    /// The hashes of all other transactions within the block.
    pub(super) standard: Vec<TransactionHash>,
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
        proposer: PublicKey,
        transfer: Vec<TransactionHash>,
        staking: Vec<TransactionHash>,
        install_upgrade: Vec<TransactionHash>,
        standard: Vec<TransactionHash>,
        rewarded_signatures: RewardedSignatures,
    ) -> Self {
        BlockBodyV2 {
            proposer,
            transfer,
            staking,
            install_upgrade,
            standard,
            rewarded_signatures,
            #[cfg(any(feature = "once_cell", test))]
            hash: OnceCell::new(),
        }
    }

    /// Returns the public key of the validator which proposed the block.
    pub fn proposer(&self) -> &PublicKey {
        &self.proposer
    }

    /// Returns the hashes of the transfer transactions within the block.
    pub fn transfer(&self) -> impl Iterator<Item = &TransactionHash> {
        self.transfer.iter()
    }

    /// Returns the hashes of the non-transfer, native transactions within the block.
    pub fn staking(&self) -> impl Iterator<Item = &TransactionHash> {
        self.staking.iter()
    }

    /// Returns the hashes of the installer/upgrader transactions within the block.
    pub fn install_upgrade(&self) -> impl Iterator<Item = &TransactionHash> {
        self.install_upgrade.iter()
    }

    /// Returns the hashes of all other transactions within the block.
    pub fn standard(&self) -> impl Iterator<Item = &TransactionHash> {
        self.standard.iter()
    }

    /// Returns all of the transaction hashes in the order in which they were executed.
    pub fn all_transactions(&self) -> impl Iterator<Item = &TransactionHash> {
        self.transfer()
            .chain(self.staking())
            .chain(self.install_upgrade())
            .chain(self.standard())
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
            proposer,
            transfer,
            staking,
            install_upgrade,
            standard,
            rewarded_signatures,
            hash: _,
        } = self;
        #[cfg(not(any(feature = "once_cell", test)))]
        let BlockBodyV2 {
            proposer,
            transfer,
            staking,
            install_upgrade,
            standard,
            rewarded_signatures,
        } = self;
        *proposer == other.proposer
            && *transfer == other.transfer
            && *staking == other.staking
            && *install_upgrade == other.install_upgrade
            && *standard == other.standard
            && *rewarded_signatures == other.rewarded_signatures
    }
}

impl Display for BlockBodyV2 {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "block body proposed by {}, {} transfers, {} non-transfer-native, {} \
            installer/upgraders, {} others",
            self.proposer,
            self.transfer.len(),
            self.staking.len(),
            self.install_upgrade.len(),
            self.standard.len()
        )
    }
}

impl ToBytes for BlockBodyV2 {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.proposer.write_bytes(writer)?;
        self.transfer.write_bytes(writer)?;
        self.staking.write_bytes(writer)?;
        self.install_upgrade.write_bytes(writer)?;
        self.standard.write_bytes(writer)?;
        self.rewarded_signatures.write_bytes(writer)?;
        Ok(())
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.proposer.serialized_length()
            + self.transfer.serialized_length()
            + self.staking.serialized_length()
            + self.install_upgrade.serialized_length()
            + self.standard.serialized_length()
            + self.rewarded_signatures.serialized_length()
    }
}

impl FromBytes for BlockBodyV2 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (proposer, bytes) = PublicKey::from_bytes(bytes)?;
        let (transfer, bytes) = Vec::<TransactionHash>::from_bytes(bytes)?;
        let (staking, bytes) = Vec::<TransactionHash>::from_bytes(bytes)?;
        let (install_upgrade, bytes) = Vec::<TransactionHash>::from_bytes(bytes)?;
        let (standard, bytes) = Vec::<TransactionHash>::from_bytes(bytes)?;
        let (rewarded_signatures, bytes) = RewardedSignatures::from_bytes(bytes)?;
        let body = BlockBodyV2 {
            proposer,
            transfer,
            staking,
            install_upgrade,
            standard,
            rewarded_signatures,
            #[cfg(any(feature = "once_cell", test))]
            hash: OnceCell::new(),
        };
        Ok((body, bytes))
    }
}
