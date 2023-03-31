use std::fmt::{self, Display, Formatter};

use datasize::DataSize;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(any(feature = "testing", test))]
use casper_types::testing::TestRng;
use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    crypto, PublicKey, SecretKey, Signature,
};

use super::DeployHash;

/// A struct containing a signature of a deploy hash and the public key of the signer.
#[derive(
    Clone, DataSize, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    signer: PublicKey,
    signature: Signature,
}

impl Approval {
    /// Creates an approval for the given deploy hash using the given secret key.
    pub fn create(hash: &DeployHash, secret_key: &SecretKey) -> Self {
        let signer = PublicKey::from(secret_key);
        let signature = crypto::sign(hash, secret_key, &signer);
        Self { signer, signature }
    }

    /// Returns the public key of the approval's signer.
    pub fn signer(&self) -> &PublicKey {
        &self.signer
    }

    /// Returns the approval signature.
    pub fn signature(&self) -> &Signature {
        &self.signature
    }
}

#[cfg(any(feature = "testing", test))]
impl Approval {
    /// Returns a random `Approval`.
    pub fn random(rng: &mut TestRng) -> Self {
        Self {
            signer: PublicKey::random(rng),
            signature: Signature::ed25519([0; Signature::ED25519_LENGTH]).unwrap(),
        }
    }
}

impl Display for Approval {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "approval({})", self.signer)
    }
}

impl ToBytes for Approval {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.signer.write_bytes(writer)?;
        self.signature.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.signer.serialized_length() + self.signature.serialized_length()
    }
}

impl FromBytes for Approval {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (signer, remainder) = PublicKey::from_bytes(bytes)?;
        let (signature, remainder) = Signature::from_bytes(remainder)?;
        let approval = Approval { signer, signature };
        Ok((approval, remainder))
    }
}
