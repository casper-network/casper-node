use hex_fmt::HexFmt;
use serde::{Deserialize, Serialize};

/// Stringified PublicKey encoding.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PublicKey(String);

impl From<casper_types::PublicKey> for PublicKey {
    fn from(public_key: casper_types::PublicKey) -> Self {
        match public_key {
            casper_types::PublicKey::Ed25519(inner) => PublicKey(format!("{:10}", HexFmt(inner))),
            casper_types::PublicKey::Secp256k1(inner) => PublicKey(format!("{:10}", HexFmt(inner))),
        }
    }
}
