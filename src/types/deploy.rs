use std::{
    convert::TryFrom,
    fmt::{self, Debug, Display, Formatter},
};

use hex_fmt::HexFmt;
use serde::{Deserialize, Serialize};

use crate::{
    components::storage::Value,
    crypto::{
        asymmetric_key::{self, PublicKey, SecretKey, Signature},
        hash::{self, Digest},
    },
    utils::DisplayIter,
};

// TODO - improve this if it's to be kept
/// Error while encoding.
#[derive(Debug)]
pub struct EncodingError;

// TODO - improve this if it's to be kept
/// Error while decoding.
#[derive(Debug)]
pub struct DecodingError;

/// The cryptographic hash of a [`Deploy`](struct.Deploy.html).
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
pub struct DeployHash(Digest);

impl DeployHash {
    /// Constructs a new `DeployHash`.
    pub fn new(hash: Digest) -> Self {
        DeployHash(hash)
    }

    /// Returns the wrapped inner hash.
    pub fn inner(&self) -> &Digest {
        &self.0
    }
}

impl Display for DeployHash {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "deploy-hash({})", self.0,)
    }
}

/// The header portion of a [`Deploy`](struct.Deploy.html).
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
pub struct DeployHeader {
    account: PublicKey,
    timestamp: u64,
    gas_price: u64,
    body_hash: Digest,
    ttl_millis: u32,
    dependencies: Vec<DeployHash>,
    chain_name: String,
}

impl Display for DeployHeader {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "deploy-header[account: {}, timestamp: {}, gas_price: {}, body_hash: {}, ttl_millis: {}, dependencies: {}, chain_name: {}]",
            self.account,
            self.timestamp,
            self.gas_price,
            self.body_hash,
            self.ttl_millis,
            DisplayIter::new(self.dependencies.iter()),
            self.chain_name,
        )
    }
}

/// A deploy; an item containing a smart contract along with the requester's signature(s).
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
pub struct Deploy {
    hash: DeployHash,
    header: DeployHeader,
    payment_code: Vec<u8>,
    session_code: Vec<u8>,
    approvals: Vec<Signature>,
}

impl Deploy {
    /// Constructs a new `Deploy`.
    // TODO(Fraser): implement properly
    pub fn new(temp: u8) -> Self {
        let hash = DeployHash::new(hash::hash(&[temp]));

        let secret_key = SecretKey::generate_ed25519();
        let account = PublicKey::from(&secret_key);

        let timestamp = u64::from(temp) + 100;
        let gas_price = u64::from(temp) + 101;
        let body_hash = hash::hash(&[temp.overflowing_add(102).0]);
        let ttl_millis = u32::from(temp) + 103;

        let dependencies = vec![
            DeployHash::new(hash::hash(&[temp.overflowing_add(104).0])),
            DeployHash::new(hash::hash(&[temp.overflowing_add(105).0])),
            DeployHash::new(hash::hash(&[temp.overflowing_add(106).0])),
        ];

        let chain_name = "Spike".to_string();

        let header = DeployHeader {
            account,
            timestamp,
            gas_price,
            body_hash,
            ttl_millis,
            dependencies,
            chain_name,
        };

        let payment_code = hash::hash(&[temp.overflowing_add(107).0]).as_ref().to_vec();
        let session_code = hash::hash(&[temp.overflowing_add(108).0]).as_ref().to_vec();

        let approvals = vec![
            asymmetric_key::sign(&[3], &secret_key, &account),
            asymmetric_key::sign(&[4], &secret_key, &account),
            asymmetric_key::sign(&[5], &secret_key, &account),
        ];

        Deploy {
            hash,
            header,
            payment_code,
            session_code,
            approvals,
        }
    }

    /// Try to convert the `Deploy` to JSON-encoded string.
    pub fn to_json(&self) -> Result<String, EncodingError> {
        let json = json::Deploy::from(self);
        serde_json::to_string(&json).map_err(|_| EncodingError)
    }

    /// Try to convert the JSON-encoded string to a `Deploy`.
    pub fn from_json(input: &str) -> Result<Self, DecodingError> {
        let json: json::Deploy = serde_json::from_str(input).map_err(|_| DecodingError)?;
        Deploy::try_from(json)
    }
}

impl Value for Deploy {
    type Id = DeployHash;
    type Header = DeployHeader;

    fn id(&self) -> &Self::Id {
        &self.hash
    }

    fn header(&self) -> &Self::Header {
        &self.header
    }
}

impl Display for Deploy {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "deploy[{}, {}, payment_code: {:10}, session_code: {:10}, approvals: {}]",
            self.hash,
            self.header,
            HexFmt(&self.payment_code),
            HexFmt(&self.session_code),
            DisplayIter::new(self.approvals.iter())
        )
    }
}

/// This module provides structs which map to the main deploy types, but which are suitable for
/// encoding to and decoding from JSON.  For all fields with binary data, this is converted to/from
/// hex strings.
mod json {
    use std::convert::{TryFrom, TryInto};

    use serde::{Deserialize, Serialize};

    use super::DecodingError;
    use crate::crypto::{
        asymmetric_key::{PublicKey, Signature},
        hash::Digest,
    };

    #[derive(Serialize, Deserialize)]
    pub(super) struct DeployHash(String);

    impl From<&super::DeployHash> for DeployHash {
        fn from(hash: &super::DeployHash) -> Self {
            DeployHash(hex::encode(hash.0))
        }
    }

    impl TryFrom<DeployHash> for super::DeployHash {
        type Error = DecodingError;

        fn try_from(hash: DeployHash) -> Result<Self, Self::Error> {
            let hash = Digest::from_hex(&hash.0).map_err(|_| DecodingError)?;
            Ok(super::DeployHash(hash))
        }
    }

    #[derive(Serialize, Deserialize)]
    pub(super) struct DeployHeader {
        account: String,
        timestamp: u64,
        gas_price: u64,
        body_hash: String,
        ttl_millis: u32,
        dependencies: Vec<DeployHash>,
        chain_name: String,
    }

    impl From<&super::DeployHeader> for DeployHeader {
        fn from(header: &super::DeployHeader) -> Self {
            DeployHeader {
                account: hex::encode(header.account.as_ref()),
                timestamp: header.timestamp,
                gas_price: header.gas_price,
                body_hash: hex::encode(header.body_hash),
                ttl_millis: header.ttl_millis,
                dependencies: header.dependencies.iter().map(Into::into).collect(),
                chain_name: header.chain_name.clone(),
            }
        }
    }

    impl TryFrom<DeployHeader> for super::DeployHeader {
        type Error = DecodingError;

        fn try_from(header: DeployHeader) -> Result<Self, Self::Error> {
            let raw_account = hex::decode(&header.account).map_err(|_| DecodingError)?;
            let account = PublicKey::ed25519_from_bytes(&raw_account).map_err(|_| DecodingError)?;

            let body_hash = Digest::from_hex(&header.body_hash).map_err(|_| DecodingError)?;

            let mut dependencies = vec![];
            for dep in header.dependencies.into_iter() {
                let hash = dep.try_into()?;
                dependencies.push(hash);
            }

            Ok(super::DeployHeader {
                account,
                timestamp: header.timestamp,
                gas_price: header.gas_price,
                body_hash,
                ttl_millis: header.ttl_millis,
                dependencies,
                chain_name: header.chain_name,
            })
        }
    }

    #[derive(Serialize, Deserialize)]
    pub(super) struct Deploy {
        hash: DeployHash,
        header: DeployHeader,
        payment_code: String,
        session_code: String,
        approvals: Vec<String>,
    }

    impl From<&super::Deploy> for Deploy {
        fn from(deploy: &super::Deploy) -> Self {
            Deploy {
                hash: (&deploy.hash).into(),
                header: (&deploy.header).into(),
                payment_code: hex::encode(&deploy.payment_code),
                session_code: hex::encode(&deploy.session_code),
                approvals: deploy.approvals.iter().map(hex::encode).collect(),
            }
        }
    }
    impl TryFrom<Deploy> for super::Deploy {
        type Error = DecodingError;

        fn try_from(deploy: Deploy) -> Result<Self, Self::Error> {
            let mut approvals = vec![];
            for approval in deploy.approvals.into_iter() {
                let raw_sig = hex::decode(&approval).map_err(|_| DecodingError)?;
                let signature =
                    Signature::ed25519_from_bytes(&raw_sig).map_err(|_| DecodingError)?;
                approvals.push(signature);
            }
            Ok(super::Deploy {
                hash: deploy.hash.try_into()?,
                header: deploy.header.try_into()?,
                payment_code: hex::decode(deploy.payment_code).map_err(|_| DecodingError)?,
                session_code: hex::decode(deploy.session_code).map_err(|_| DecodingError)?,
                approvals,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_roundtrip() {
        let deploy = Deploy::new(1);
        let json = deploy.to_json().unwrap();
        let decoded = Deploy::from_json(&json).unwrap();
        assert_eq!(deploy, decoded);
    }
}
