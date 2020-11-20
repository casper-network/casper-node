use lazy_static::lazy_static;

use casper_execution_engine::{core::engine_state::GenesisAccount, shared::motes::Motes};

use crate::{
    crypto::asymmetric_key::{PublicKey, SecretKey},
    types::Timestamp,
    utils::Loadable,
    Chainspec,
};

lazy_static! {
    pub static ref ALICE_RAW_SECRET: Vec<u8> = vec![0; SecretKey::ED25519_LENGTH];
    pub static ref ALICE_SECRET_KEY: SecretKey =
        SecretKey::ed25519_from_bytes(&*ALICE_RAW_SECRET).unwrap();
    pub static ref ALICE_PUBLIC_KEY: PublicKey = PublicKey::from(ALICE_SECRET_KEY.deref());
    pub static ref BOB_RAW_SECRET: Vec<u8> = vec![1; SecretKey::ED25519_LENGTH];
    pub static ref BOB_PRIVATE_KEY: SecretKey =
        SecretKey::ed25519_from_bytes(&*BOB_RAW_SECRET).unwrap();
    pub static ref BOB_PUBLIC_KEY: PublicKey = PublicKey::from(BOB_PRIVATE_KEY.deref());
}

/// Loads the local chainspec and overrides timestamp and genesis account with the given stakes.
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

    // Every era has exactly three blocks.
    chainspec.genesis.highway_config.minimum_era_height = 2;
    chainspec.genesis.highway_config.era_duration = 0.into();
    chainspec
}
