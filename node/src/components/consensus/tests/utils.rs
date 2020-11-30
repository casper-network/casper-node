use once_cell::sync::Lazy;

use casper_execution_engine::{core::engine_state::GenesisAccount, shared::motes::Motes};

use crate::{
    crypto::asymmetric_key::{PublicKey, SecretKey},
    types::Timestamp,
    utils::Loadable,
    Chainspec,
};

pub static ALICE_SECRET_KEY: Lazy<SecretKey> =
    Lazy::new(|| SecretKey::ed25519_from_bytes(&[0; SecretKey::ED25519_LENGTH]).unwrap());
pub static ALICE_PUBLIC_KEY: Lazy<PublicKey> = Lazy::new(|| PublicKey::from(&*ALICE_SECRET_KEY));
pub static BOB_PRIVATE_KEY: Lazy<SecretKey> =
    Lazy::new(|| SecretKey::ed25519_from_bytes(&[1; SecretKey::ED25519_LENGTH]).unwrap());
pub static BOB_PUBLIC_KEY: Lazy<PublicKey> = Lazy::new(|| PublicKey::from(&*BOB_PRIVATE_KEY));

/// Loads the local chainspec and overrides timestamp and genesis account with the given stakes.
/// The test `Chainspec` returned has eras with exactly two blocks.
pub fn new_test_chainspec(stakes: Vec<(PublicKey, u64)>) -> Chainspec {
    let mut chainspec = Chainspec::from_resources("test/valid/chainspec.toml");
    chainspec.genesis.accounts = stakes
        .into_iter()
        .map(|(pk, stake)| {
            let motes = Motes::new(stake.into());
            GenesisAccount::new(pk.into(), pk.to_account_hash(), motes, motes)
        })
        .collect();
    chainspec.genesis.timestamp = Timestamp::now();
    chainspec.genesis.highway_config.genesis_era_start_timestamp = chainspec.genesis.timestamp;

    // Every era has exactly two blocks.
    chainspec.genesis.highway_config.minimum_era_height = 2;
    chainspec.genesis.highway_config.era_duration = 0.into();
    chainspec
}
