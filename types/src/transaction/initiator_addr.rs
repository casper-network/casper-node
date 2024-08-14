use super::serialization::{BinaryPayload, CalltableFromBytes, CalltableToBytes};
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    account::AccountHash,
    bytesrepr::{self, ToBytes},
    AsymmetricType, PublicKey,
};
use alloc::vec::Vec;
use core::fmt::{self, Debug, Display, Formatter};
#[cfg(feature = "datasize")]
use datasize::DataSize;
use macros::{CalltableFromBytes, CalltableToBytes};
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The address of the initiator of a [`TransactionV1`].
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "The address of the initiator of a TransactionV1.")
)]
#[serde(deny_unknown_fields)]
#[derive(CalltableToBytes, CalltableFromBytes)]
pub enum InitiatorAddr {
    #[calltable(variant_index = 0)]
    /// The public key of the initiator.
    PublicKey(#[calltable(field_index = 1)] PublicKey),
    #[calltable(variant_index = 1)]
    /// The account hash derived from the public key of the initiator.
    AccountHash(#[calltable(field_index = 1)] AccountHash),
}

impl InitiatorAddr {
    /// Gets the account hash.
    pub fn account_hash(&self) -> AccountHash {
        match self {
            InitiatorAddr::PublicKey(public_key) => public_key.to_account_hash(),
            InitiatorAddr::AccountHash(hash) => *hash,
        }
    }

    /// Returns a random `InitiatorAddr`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..=1) {
            0 => InitiatorAddr::PublicKey(PublicKey::random(rng)),
            1 => InitiatorAddr::AccountHash(rng.gen()),
            _ => unreachable!(),
        }
    }
}

impl From<PublicKey> for InitiatorAddr {
    fn from(public_key: PublicKey) -> Self {
        InitiatorAddr::PublicKey(public_key)
    }
}

impl From<AccountHash> for InitiatorAddr {
    fn from(account_hash: AccountHash) -> Self {
        InitiatorAddr::AccountHash(account_hash)
    }
}

impl Display for InitiatorAddr {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            InitiatorAddr::PublicKey(public_key) => {
                write!(formatter, "public key {}", public_key.to_hex())
            }
            InitiatorAddr::AccountHash(account_hash) => {
                write!(formatter, "account hash {}", account_hash)
            }
        }
    }
}

impl Debug for InitiatorAddr {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            InitiatorAddr::PublicKey(public_key) => formatter
                .debug_tuple("PublicKey")
                .field(public_key)
                .finish(),
            InitiatorAddr::AccountHash(account_hash) => formatter
                .debug_tuple("AccountHash")
                .field(account_hash)
                .finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gens::initiator_addr_arb;
    use proptest::prelude::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        for _ in 0..10 {
            bytesrepr::test_serialization_roundtrip(&InitiatorAddr::random(rng));
        }
    }

    proptest! {
        #[test]
        fn generative_bytesrepr_roundtrip(val in initiator_addr_arb()) {
            bytesrepr::test_serialization_roundtrip(&val);
        }
    }
}
