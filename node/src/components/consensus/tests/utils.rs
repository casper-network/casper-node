use once_cell::sync::Lazy;

use casper_execution_engine::{core::engine_state::GenesisAccount, shared::motes::Motes};
use casper_types::{PublicKey, SecretKey, U512};

use crate::{types::Timestamp, utils::Loadable, Chainspec};

pub static ALICE_SECRET_KEY: Lazy<SecretKey> =
    Lazy::new(|| SecretKey::ed25519([0; SecretKey::ED25519_LENGTH]));
pub static ALICE_PUBLIC_KEY: Lazy<PublicKey> = Lazy::new(|| PublicKey::from(&*ALICE_SECRET_KEY));

pub static BOB_PRIVATE_KEY: Lazy<SecretKey> =
    Lazy::new(|| SecretKey::ed25519([1; SecretKey::ED25519_LENGTH]));
pub static BOB_PUBLIC_KEY: Lazy<PublicKey> = Lazy::new(|| PublicKey::from(&*BOB_PRIVATE_KEY));

/// Loads the local chainspec and overrides timestamp and genesis account with the given stakes.
/// The test `Chainspec` returned has eras with exactly two blocks.
pub fn new_test_chainspec<I, T>(stakes: I) -> Chainspec
where
    I: IntoIterator<Item = (PublicKey, T)>,
    T: Into<U512>,
{
    let mut chainspec = Chainspec::from_resources("test/valid/chainspec.toml");
    chainspec.genesis.accounts = stakes
        .into_iter()
        .map(|(pk, stake)| {
            let motes = Motes::new(stake.into());
            GenesisAccount::new(pk, pk.to_account_hash(), motes, motes)
        })
        .collect();
    chainspec.genesis.timestamp = Timestamp::now();

    // Every era has exactly two blocks.
    chainspec.genesis.highway_config.minimum_era_height = 2;
    chainspec.genesis.highway_config.era_duration = 0.into();
    chainspec
}
