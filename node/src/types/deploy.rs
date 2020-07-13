use std::{
    convert::TryFrom,
    fmt::{self, Debug, Display, Formatter},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    components::{
        contract_runtime::core::engine_state::executable_deploy_item::ExecutableDeployItem,
        storage::Value,
    },
    crypto::{
        asymmetric_key::{PublicKey, Signature},
        hash::Digest,
    },
    utils::DisplayIter,
};

// TODO - improve this if it's to be kept
/// Error while encoding.
#[derive(Debug, Error)]
pub struct EncodingError;

impl Display for EncodingError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "encoding error")
    }
}

// TODO - improve this if it's to be kept
/// Error while decoding.
#[derive(Debug, Error)]
pub struct DecodingError;

impl Display for DecodingError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "decoding error")
    }
}

/// The cryptographic hash of a [`Deploy`](struct.Deploy.html).
#[derive(
    Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug, Default,
)]
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

impl From<Digest> for DeployHash {
    fn from(digest: Digest) -> Self {
        Self(digest)
    }
}

/// The header portion of a [`Deploy`](struct.Deploy.html).
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
pub struct DeployHeader {
    /// The account within which the deploy will be run
    pub account: PublicKey,
    /// When the deploy was created
    pub timestamp: u64,
    /// Price per gas unit for this deploy
    pub gas_price: u64,
    /// Hash of the WASM code
    pub body_hash: Digest,
    /// How long the deploy will stay valid
    pub ttl_millis: u32,
    /// Other deploys that have to be run before this one
    pub dependencies: Vec<DeployHash>,
    /// Which chain the deploy is supposed to be run on
    pub chain_name: String,
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
    payment: ExecutableDeployItem,
    session: ExecutableDeployItem,
    approvals: Vec<Signature>,
}

impl Deploy {
    /// Constructs a new `Deploy`.
    pub fn new(
        hash: DeployHash,
        header: DeployHeader,
        payment: ExecutableDeployItem,
        session: ExecutableDeployItem,
        approvals: Vec<Signature>,
    ) -> Deploy {
        Deploy {
            hash,
            header,
            payment,
            session,
            approvals,
        }
    }

    /// Returns the `DeployHash` identifying this `Deploy`.
    pub fn id(&self) -> &DeployHash {
        &self.hash
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

    /// Returns the `ExecutableDeployItem` for payment code.
    pub fn payment(&self) -> &ExecutableDeployItem {
        &self.payment
    }

    /// Returns the `ExecutableDeployItem` for session code.
    pub fn session(&self) -> &ExecutableDeployItem {
        &self.session
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

    fn take_header(self) -> Self::Header {
        self.header
    }
}

impl Display for Deploy {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "deploy[{}, {}, payment_code: {:?}, session_code: {:?}, approvals: {}]",
            self.hash,
            self.header,
            self.payment,
            self.session,
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
    use crate::{
        components::contract_runtime::core::engine_state::executable_deploy_item::ExecutableDeployItem,
        crypto::{
            asymmetric_key::{PublicKey, Signature},
            hash::Digest,
        },
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
        payment: ExecutableDeployItem,
        session: ExecutableDeployItem,
        approvals: Vec<String>,
    }

    impl From<&super::Deploy> for Deploy {
        fn from(deploy: &super::Deploy) -> Self {
            Deploy {
                hash: (&deploy.hash).into(),
                header: (&deploy.header).into(),
                payment: deploy.payment.clone(),
                session: deploy.session.clone(),
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
                payment: deploy.payment,
                session: deploy.session,
                approvals,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::asymmetric_key::{self, SecretKey};

    #[test]
    fn json_roundtrip() {
        let secret_key = SecretKey::generate_ed25519();
        let public_key = PublicKey::from(&secret_key);

        let deploy_hash: Digest = [1; 32].into();

        let deploy = Deploy::new(
            DeployHash::from(deploy_hash),
            DeployHeader {
                account: PublicKey::new_ed25519([42; 32]).unwrap(),
                timestamp: 100,
                gas_price: 101,
                body_hash: [43; 32].into(),
                ttl_millis: 102,
                dependencies: vec![],
                chain_name: "Foo".to_owned(),
            },
            ExecutableDeployItem::ModuleBytes {
                module_bytes: b"This is WASM of a module".to_vec(),
                args: b"foo".to_vec(),
            },
            ExecutableDeployItem::ModuleBytes {
                module_bytes: b"This is WASM of a module".to_vec(),
                args: b"bar".to_vec(),
            },
            vec![asymmetric_key::sign(b"Message", &secret_key, &public_key)],
        );
        let json = deploy.to_json().unwrap();
        let decoded = Deploy::from_json(&json).unwrap();
        assert_eq!(deploy, decoded);
    }
}
