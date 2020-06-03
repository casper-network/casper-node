use std::fmt::{self, Debug, Display, Formatter};

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
        write!(formatter, "DeployHash({})", self.0,)
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
            "DeployHeader {{ account: {}, timestamp: {}, gas_price: {}, body_hash: {}, ttl_millis: {}, dependencies: {}, chain_name: {} }}",
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
            "Deploy {{ hash: {}, {}, payment_code: {:10}, session_code: {:10}, approvals: {} }}",
            self.hash,
            self.header,
            HexFmt(&self.payment_code),
            HexFmt(&self.session_code),
            DisplayIter::new(self.approvals.iter())
        )
    }
}
