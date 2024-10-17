#[cfg(any(feature = "testing", test))]
use rand::Rng;

#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{account::AccountHash, PublicKey, URef};

/// The various types which can be used as the `target` runtime argument of a native transfer.
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Debug)]
pub enum TransferTarget {
    /// A public key.
    PublicKey(PublicKey),
    /// An account hash.
    AccountHash(AccountHash),
    /// A URef.
    URef(URef),
}

impl TransferTarget {
    /// Returns a random `TransferTarget`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..3) {
            0 => TransferTarget::PublicKey(PublicKey::random(rng)),
            1 => TransferTarget::AccountHash(rng.gen()),
            2 => TransferTarget::URef(rng.gen()),
            _ => unreachable!(),
        }
    }
}

impl From<PublicKey> for TransferTarget {
    fn from(public_key: PublicKey) -> Self {
        Self::PublicKey(public_key)
    }
}

impl From<AccountHash> for TransferTarget {
    fn from(account_hash: AccountHash) -> Self {
        Self::AccountHash(account_hash)
    }
}

impl From<URef> for TransferTarget {
    fn from(uref: URef) -> Self {
        Self::URef(uref)
    }
}
