use crate::{InitiatorAddr, PublicKey, SecretKey};

/// Used when constructing a deploy or transaction.
#[derive(Debug)]
pub(super) enum InitiatorAddrAndSecretKey<'a> {
    /// Provides both the initiator address and the secret key (not necessarily for the same
    /// initiator address) used to sign the deploy or transaction.
    Both {
        /// The initiator address of the account.
        initiator_addr: InitiatorAddr,
        /// The secret key used to sign the deploy or transaction.
        secret_key: &'a SecretKey,
    },
    /// The initiator address only (no secret key).  The deploy or transaction will be created
    /// unsigned.
    InitiatorAddr(InitiatorAddr),
    /// The initiator address will be derived from the provided secret key, and the deploy or
    /// transaction will be signed by the same secret key.
    SecretKey(&'a SecretKey),
}

impl<'a> InitiatorAddrAndSecretKey<'a> {
    pub fn initiator_addr(&self) -> InitiatorAddr {
        match self {
            InitiatorAddrAndSecretKey::Both { initiator_addr, .. }
            | InitiatorAddrAndSecretKey::InitiatorAddr(initiator_addr) => initiator_addr.clone(),
            InitiatorAddrAndSecretKey::SecretKey(secret_key) => {
                InitiatorAddr::PublicKey(PublicKey::from(*secret_key))
            }
        }
    }

    pub fn secret_key(&self) -> Option<&SecretKey> {
        match self {
            InitiatorAddrAndSecretKey::Both { secret_key, .. }
            | InitiatorAddrAndSecretKey::SecretKey(secret_key) => Some(secret_key),
            InitiatorAddrAndSecretKey::InitiatorAddr(_) => None,
        }
    }
}
