use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "once_cell", test))]
use once_cell::sync::OnceCell;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(all(feature = "std", feature = "json-schema"))]
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    past_finality_signatures::PastFinalitySignatures,
    DeployHash, Digest, PublicKey,
};

#[derive(Clone, Eq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[schemars(description = "The body portion of a block.")]
pub struct BlockBodyV2 {
    /// The public key of the validator which proposed the block.
    pub(super) proposer: PublicKey,
    /// The deploy hashes of the non-transfer deploys within the block.
    pub(super) deploy_hashes: Vec<DeployHash>,
    /// The deploy hashes of the transfers within the block.
    pub(super) transfer_hashes: Vec<DeployHash>,
    /// The past finality signatures.
    pub(super) past_finality_signatures: PastFinalitySignatures,
    #[serde(skip)]
    #[cfg_attr(
        all(any(feature = "once_cell", test), feature = "datasize"),
        data_size(skip)
    )]
    #[cfg(any(feature = "once_cell", test))]
    pub(super) hash: OnceCell<Digest>,
}

impl BlockBodyV2 {
    /// Constructs a new `BlockBody`.
    pub(crate) fn new(
        proposer: PublicKey,
        deploy_hashes: Vec<DeployHash>,
        transfer_hashes: Vec<DeployHash>,
    ) -> Self {
        BlockBodyV2 {
            proposer,
            deploy_hashes,
            transfer_hashes,
            past_finality_signatures: Default::default(),
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

    /// Returns the past finality signatures stored in the block.
    pub fn past_finality_signatures(&self) -> &PastFinalitySignatures {
        &self.past_finality_signatures
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

impl PartialEq for BlockBodyV2 {
    fn eq(&self, other: &BlockBodyV2) -> bool {
        // Destructure to make sure we don't accidentally omit fields.
        #[cfg(any(feature = "once_cell", test))]
        let BlockBodyV2 {
            proposer,
            deploy_hashes,
            transfer_hashes,
            past_finality_signatures,
            hash: _,
        } = self;
        #[cfg(not(any(feature = "once_cell", test)))]
        let BlockBodyV2 {
            proposer,
            deploy_hashes,
            transfer_hashes,
            past_finality_signatures,
        } = self;
        *proposer == other.proposer
            && *deploy_hashes == other.deploy_hashes
            && *transfer_hashes == other.transfer_hashes
            && *past_finality_signatures == other.past_finality_signatures
    }
}

impl Display for BlockBodyV2 {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "block body proposed by {}, {} deploys, {} transfers, {} past finality signatures",
            self.proposer,
            self.deploy_hashes.len(),
            self.transfer_hashes.len(),
            self.past_finality_signatures.0.len()
        )
    }
}

impl ToBytes for BlockBodyV2 {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.proposer.write_bytes(writer)?;
        self.deploy_hashes.write_bytes(writer)?;
        self.transfer_hashes.write_bytes(writer)?;
        self.past_finality_signatures.write_bytes(writer)
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
            + self.past_finality_signatures.serialized_length()
    }
}

impl FromBytes for BlockBodyV2 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (proposer, bytes) = PublicKey::from_bytes(bytes)?;
        let (deploy_hashes, bytes) = Vec::<DeployHash>::from_bytes(bytes)?;
        let (transfer_hashes, bytes) = Vec::<DeployHash>::from_bytes(bytes)?;
        let (past_finality_signatures, bytes) = PastFinalitySignatures::from_bytes(bytes)?;
        let body = BlockBodyV2 {
            proposer,
            deploy_hashes,
            transfer_hashes,
            past_finality_signatures,
            #[cfg(any(feature = "once_cell", test))]
            hash: OnceCell::new(),
        };
        Ok((body, bytes))
    }
}
