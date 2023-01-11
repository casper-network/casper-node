use std::sync::Arc;

use num::Zero;
use once_cell::sync::Lazy;

use casper_types::{
    system::auction::DelegationRate, Motes, PublicKey, SecretKey, TimeDiff, Timestamp, U512,
};

use crate::{
    tls::{KeyFingerprint, Sha512},
    types::{
        chainspec::{AccountConfig, AccountsConfig, ValidatorConfig},
        ActivationPoint, Chainspec, ChainspecRawBytes, NodeId,
    },
    utils::Loadable,
};

pub static ALICE_SECRET_KEY: Lazy<Arc<SecretKey>> =
    Lazy::new(|| Arc::new(SecretKey::ed25519_from_bytes([0; SecretKey::ED25519_LENGTH]).unwrap()));
pub static ALICE_PUBLIC_KEY: Lazy<PublicKey> = Lazy::new(|| PublicKey::from(&**ALICE_SECRET_KEY));
pub static ALICE_NODE_ID: Lazy<NodeId> = Lazy::new(|| {
    NodeId::from(KeyFingerprint::from(Sha512::new(match *ALICE_PUBLIC_KEY {
        PublicKey::Ed25519(pub_key) => pub_key,
        _ => panic!("ALICE_PUBLIC_KEY is Ed25519"),
    })))
});

pub static BOB_SECRET_KEY: Lazy<Arc<SecretKey>> =
    Lazy::new(|| Arc::new(SecretKey::ed25519_from_bytes([1; SecretKey::ED25519_LENGTH]).unwrap()));
pub static BOB_PUBLIC_KEY: Lazy<PublicKey> = Lazy::new(|| PublicKey::from(&**BOB_SECRET_KEY));
pub static BOB_NODE_ID: Lazy<NodeId> = Lazy::new(|| {
    NodeId::from(KeyFingerprint::from(Sha512::new(match *BOB_PUBLIC_KEY {
        PublicKey::Ed25519(pub_key) => pub_key,
        _ => panic!("BOB_PUBLIC_KEY is Ed25519"),
    })))
});

pub static CAROL_SECRET_KEY: Lazy<Arc<SecretKey>> =
    Lazy::new(|| Arc::new(SecretKey::ed25519_from_bytes([2; SecretKey::ED25519_LENGTH]).unwrap()));
pub static CAROL_PUBLIC_KEY: Lazy<PublicKey> = Lazy::new(|| PublicKey::from(&**CAROL_SECRET_KEY));

/// Loads the local chainspec and overrides timestamp and genesis account with the given stakes.
/// The test `Chainspec` returned has eras with exactly two blocks.
pub fn new_test_chainspec<I, T>(stakes: I) -> Chainspec
where
    I: IntoIterator<Item = (PublicKey, T)>,
    T: Into<U512>,
{
    let (mut chainspec, _) = <(Chainspec, ChainspecRawBytes)>::from_resources("local");
    let accounts = stakes
        .into_iter()
        .map(|(pk, stake)| {
            let motes = Motes::new(stake.into());
            let validator_config = ValidatorConfig::new(motes, DelegationRate::zero());
            AccountConfig::new(pk, motes, Some(validator_config))
        })
        .collect();
    let delegators = vec![];
    chainspec.network_config.accounts_config = AccountsConfig::new(accounts, delegators);
    chainspec.protocol_config.activation_point = ActivationPoint::Genesis(Timestamp::now());

    // Every era has exactly two blocks.
    chainspec.core_config.minimum_era_height = 2;
    chainspec.core_config.era_duration = TimeDiff::from_millis(0);
    chainspec
}
