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
    bytesrepr::{self, FromBytes, ToBytes},
    DeployHash, Digest, PublicKey,
};

/// The body portion of a block. Version 1.
#[derive(Clone, Eq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct BlockBodyV1 {
    /// The public key of the validator which proposed the block.
    pub(super) proposer: PublicKey,
    /// The deploy hashes of the non-transfer deploys within the block.
    pub(super) deploy_hashes: Vec<DeployHash>,
    /// The deploy hashes of the transfers within the block.
    pub(super) transfer_hashes: Vec<DeployHash>,
    #[serde(skip)]
    #[cfg_attr(
        all(any(feature = "once_cell", test), feature = "datasize"),
        data_size(skip)
    )]
    #[cfg(any(feature = "once_cell", test))]
    pub(super) hash: OnceCell<Digest>,
}

impl BlockBodyV1 {
    /// Constructs a new `BlockBody`.
    pub(crate) fn new(
        proposer: PublicKey,
        deploy_hashes: Vec<DeployHash>,
        transfer_hashes: Vec<DeployHash>,
    ) -> Self {
        BlockBodyV1 {
            proposer,
            deploy_hashes,
            transfer_hashes,
            #[cfg(any(feature = "once_cell", test))]
            hash: OnceCell::new(),
        }
    }

    /// Returns the public key of the validator which proposed the block.
    pub fn proposer(&self) -> &PublicKey {
        &self.proposer
    }

    /// Returns the deploy hashes of the non-transfer deploys within the block.
    pub fn deploy_hashes(&self) -> &[DeployHash] {
        &self.deploy_hashes
    }

    /// Returns the deploy hashes of the transfers within the block.
    pub fn transfer_hashes(&self) -> &[DeployHash] {
        &self.transfer_hashes
    }

    /// Returns the deploy and transfer hashes in the order in which they were executed.
    pub fn deploy_and_transfer_hashes(&self) -> impl Iterator<Item = &DeployHash> {
        self.deploy_hashes()
            .iter()
            .chain(self.transfer_hashes().iter())
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
}

impl PartialEq for BlockBodyV1 {
    fn eq(&self, other: &BlockBodyV1) -> bool {
        // Destructure to make sure we don't accidentally omit fields.
        #[cfg(any(feature = "once_cell", test))]
        let BlockBodyV1 {
            proposer,
            deploy_hashes,
            transfer_hashes,
            hash: _,
        } = self;
        #[cfg(not(any(feature = "once_cell", test)))]
        let BlockBodyV1 {
            proposer,
            deploy_hashes,
            transfer_hashes,
        } = self;
        *proposer == other.proposer
            && *deploy_hashes == other.deploy_hashes
            && *transfer_hashes == other.transfer_hashes
    }
}

impl Display for BlockBodyV1 {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "block body proposed by {}, {} deploys, {} transfers",
            self.proposer,
            self.deploy_hashes.len(),
            self.transfer_hashes.len()
        )
    }
}

impl ToBytes for BlockBodyV1 {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.proposer.write_bytes(writer)?;
        self.deploy_hashes.write_bytes(writer)?;
        self.transfer_hashes.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.proposer.serialized_length()
            + self.deploy_hashes.serialized_length()
            + self.transfer_hashes.serialized_length()
    }
}

impl FromBytes for BlockBodyV1 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (proposer, bytes) = PublicKey::from_bytes(bytes)?;
        let (deploy_hashes, bytes) = Vec::<DeployHash>::from_bytes(bytes)?;
        let (transfer_hashes, bytes) = Vec::<DeployHash>::from_bytes(bytes)?;
        let body = BlockBodyV1 {
            proposer,
            deploy_hashes,
            transfer_hashes,
            #[cfg(any(feature = "once_cell", test))]
            hash: OnceCell::new(),
        };
        Ok((body, bytes))
    }
}
